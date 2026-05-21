use crate::logs::append_log_line;
use crate::settings::{Backend, CompanionSettings};
use crate::state::{ModelStatus, RuntimeState, ServiceStatus};
use serde::Deserialize;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const ASR_HOST: &str = "127.0.0.1";
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_HEALTH_TIMEOUT: Duration = Duration::from_secs(15);
const STARTUP_HEALTH_INTERVAL: Duration = Duration::from_millis(500);
const RUNNING_POLL_INTERVAL: Duration = Duration::from_secs(2);
const LOCAL_HTTP_TIMEOUT: Duration = Duration::from_millis(750);
const LOCAL_MODEL_LOAD_TIMEOUT: Duration = Duration::from_secs(120);
const MODEL_LOAD_POLL_TIMEOUT: Duration = Duration::from_secs(120);
const MODEL_LOAD_POLL_INTERVAL: Duration = Duration::from_millis(500);
const MAX_HEALTH_FAILURES: u8 = 3;
const MAX_RECENT_LOGS: usize = 20;

pub(crate) type ProcessManagerState = Mutex<ProcessManager>;

#[derive(Debug)]
pub(crate) struct ProcessManager {
    child: Option<Child>,
    status: ServiceStatus,
    model_status: ModelStatus,
    base_url: Option<String>,
    pid: Option<u32>,
    port: Option<u16>,
    resolved_model_id: Option<String>,
    backend: Option<Backend>,
    last_error: Option<String>,
    last_exit_code: Option<i32>,
    consecutive_health_failures: u8,
    last_poll_at: Option<Instant>,
    companion_log_path: Option<PathBuf>,
    asr_log_path: Option<PathBuf>,
    recent_logs: Vec<String>,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self {
            child: None,
            status: ServiceStatus::Stopped,
            model_status: ModelStatus::Unknown,
            base_url: None,
            pid: None,
            port: None,
            resolved_model_id: None,
            backend: None,
            last_error: None,
            last_exit_code: None,
            consecutive_health_failures: 0,
            last_poll_at: None,
            companion_log_path: None,
            asr_log_path: None,
            recent_logs: Vec::new(),
        }
    }
}

impl ProcessManager {
    pub fn set_log_paths(&mut self, companion_log_path: PathBuf, asr_log_path: PathBuf) {
        self.companion_log_path = Some(companion_log_path);
        self.asr_log_path = Some(asr_log_path);
    }

    pub fn start(&mut self, settings: &CompanionSettings) {
        self.refresh_child_status();

        if self.child.is_some() {
            self.push_log(
                "Start requested while a Companion-owned ASR service process still exists.",
            );
            return;
        }

        self.status = ServiceStatus::Starting;
        self.model_status = ModelStatus::Unknown;
        self.last_error = None;
        self.last_exit_code = None;
        self.pid = None;
        self.port = Some(settings.preferred_port);
        self.base_url = Some(base_url(settings.preferred_port));
        self.resolved_model_id = Some(settings.resolved_model_id());
        self.backend = Some(settings.backend);
        self.consecutive_health_failures = 0;
        self.last_poll_at = None;

        if local_port_is_listening(settings.preferred_port) {
            if self.attach_existing_service(settings.preferred_port) {
                return;
            }

            self.fail_start(format!(
                "Port {} is already in use by another process, but it is not a healthy EchoNote ASR service. Stop the process using this port or choose a different port.",
                settings.preferred_port
            ));
            return;
        }

        let service_dir = match resolve_service_dir(&settings.asr_service_path) {
            Ok(service_dir) => service_dir,
            Err(error) => {
                self.fail_start(error);
                return;
            }
        };

        let python_path = expand_tilde(&settings.python_path);
        let stdout = match self.asr_log_stdio() {
            Ok(stdout) => stdout,
            Err(error) => {
                self.fail_start(error);
                return;
            }
        };
        let stderr = match self.asr_log_stdio() {
            Ok(stderr) => stderr,
            Err(error) => {
                self.fail_start(error);
                return;
            }
        };
        let resolved_model_id = settings.resolved_model_id();
        let mut command = Command::new(&python_path);
        command
            .current_dir(&service_dir)
            .arg("-m")
            .arg("echonote_asr")
            .arg("--host")
            .arg(ASR_HOST)
            .arg("--port")
            .arg(settings.preferred_port.to_string())
            .arg("--model")
            .arg(&resolved_model_id)
            .arg("--backend")
            .arg(backend_cli_arg(settings.backend))
            .arg("--log-level")
            .arg("info")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);

        match command.spawn() {
            Ok(child) => {
                let pid = child.id();
                self.child = Some(child);
                self.pid = Some(pid);
                self.push_log(format!(
                    "Started ASR service pid {pid} on {url} with backend {backend}; waiting for health.",
                    url = base_url(settings.preferred_port),
                    backend = backend_cli_arg(settings.backend)
                ));
                self.wait_for_startup_health(settings.preferred_port);
            }
            Err(error) => {
                self.fail_start(format!(
                    "Failed to start ASR service with '{}': {error}",
                    python_path.display()
                ));
            }
        }
    }

    pub fn stop(&mut self) {
        self.refresh_child_status();

        let Some(mut child) = self.child.take() else {
            if self.stop_external_service_if_available() {
                return;
            }

            self.status = ServiceStatus::Stopped;
            self.pid = None;
            self.push_log("Stop requested with no Companion-owned ASR service running.");
            return;
        };

        self.status = ServiceStatus::Stopping;
        self.pid = Some(child.id());
        self.consecutive_health_failures = 0;
        self.last_poll_at = None;

        let port = self.port.unwrap_or(8765);
        match request_shutdown(port) {
            Ok(()) => self.push_log(format!("Sent shutdown request to {}.", base_url(port))),
            Err(error) => self.push_log(format!("Shutdown request failed: {error}")),
        }

        match wait_for_child_exit(&mut child, STOP_TIMEOUT) {
            Ok(Some(exit_status)) => {
                self.finish_stopped(exit_status);
            }
            Ok(None) => {
                self.push_log(
                    "ASR service did not exit before timeout; terminating child process.",
                );
                if let Err(error) = child.kill() {
                    self.push_log(format!("Terminate fallback failed: {error}"));
                }

                match child.wait() {
                    Ok(exit_status) => self.finish_stopped(exit_status),
                    Err(error) => {
                        self.fail_stop(format!("Failed to wait for ASR service exit: {error}"))
                    }
                }
            }
            Err(error) => {
                self.fail_stop(error);
            }
        }
    }

    pub fn restart(&mut self, settings: &CompanionSettings) {
        if self.child.is_some() {
            self.stop();
        }
        self.start(settings);
    }

    pub fn load_model(&mut self, settings: &CompanionSettings) {
        self.refresh_child_status();

        if self.status != ServiceStatus::Running {
            let message = "Load Model requires a running ASR service.".to_string();
            self.last_error = Some(message.clone());
            self.push_log(message);
            return;
        }

        let Some(port) = self.port else {
            let message = "Load Model failed because the ASR service port is unknown.".to_string();
            self.last_error = Some(message.clone());
            self.push_log(message);
            return;
        };

        let model_id = settings.resolved_model_id();
        self.model_status = ModelStatus::Loading;
        self.resolved_model_id = Some(model_id.clone());
        self.last_error = None;
        self.push_log(format!("Loading ASR model {model_id}."));

        match request_model_load(port, &model_id) {
            Ok(model_load) => {
                self.model_status = map_model_status(&model_load.status);
                self.resolved_model_id = Some(model_load.model_id);
                self.last_poll_at = None;
                if self.model_status == ModelStatus::Loading {
                    self.wait_for_model_load(port);
                } else if self.model_status == ModelStatus::Ready {
                    self.push_log("ASR model is ready.");
                }
            }
            Err(error) => {
                self.model_status = ModelStatus::Error;
                let message = format!("Failed to load ASR model: {error}");
                self.last_error = Some(message.clone());
                self.push_log(message);
            }
        }
    }

    pub fn shutdown_on_app_exit(&mut self) {
        self.refresh_child_status();

        if self.child.is_some()
            || matches!(
                self.status,
                ServiceStatus::Starting | ServiceStatus::Running | ServiceStatus::Stopping
            )
        {
            self.push_log("Companion app is exiting; stopping ASR service.");
            self.stop();
        }
    }

    pub fn snapshot(&mut self, settings: &CompanionSettings) -> RuntimeState {
        self.refresh_child_status();
        self.poll_running_status_if_due();

        let mut runtime = RuntimeState::from_settings(settings);
        runtime.service_status = self.status;
        runtime.model_status = self.model_status;
        runtime.base_url = self
            .base_url
            .clone()
            .or_else(|| Some(base_url(settings.preferred_port)));
        runtime.port = self.port.unwrap_or(settings.preferred_port);
        runtime.pid = self.pid;
        runtime.resolved_model_id = self
            .resolved_model_id
            .clone()
            .unwrap_or_else(|| settings.resolved_model_id());
        runtime.backend = self.backend.unwrap_or(settings.backend);
        runtime.last_error = self.last_error.clone();
        runtime.last_exit_code = self.last_exit_code;

        if !self.recent_logs.is_empty() {
            runtime.recent_logs = self.recent_logs.clone();
        }

        runtime
    }

    fn refresh_child_status(&mut self) {
        let wait_result = match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => return,
        };

        match wait_result {
            Ok(Some(exit_status)) => {
                self.child = None;
                self.pid = None;
                self.last_exit_code = exit_status.code();
                self.consecutive_health_failures = 0;
                self.last_poll_at = None;
                if self.status != ServiceStatus::Stopping {
                    self.status = ServiceStatus::Error;
                    let error = format!(
                        "ASR service exited unexpectedly with {}.",
                        format_exit_status(exit_status)
                    );
                    self.last_error = Some(error.clone());
                    self.push_log(error);
                } else {
                    self.status = ServiceStatus::Stopped;
                    self.push_log(format!(
                        "ASR service stopped with {}.",
                        format_exit_status(exit_status)
                    ));
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.child = None;
                self.pid = None;
                self.status = ServiceStatus::Error;
                let message = format!("Failed to inspect ASR service process: {error}");
                self.last_error = Some(message.clone());
                self.push_log(message);
            }
        }
    }

    fn fail_start(&mut self, error: String) {
        self.child = None;
        self.pid = None;
        self.status = ServiceStatus::Error;
        self.last_error = Some(error.clone());
        self.consecutive_health_failures = 0;
        self.last_poll_at = None;
        self.push_log(error);
    }

    fn fail_stop(&mut self, error: String) {
        self.child = None;
        self.pid = None;
        self.status = ServiceStatus::Error;
        self.last_error = Some(error.clone());
        self.consecutive_health_failures = 0;
        self.last_poll_at = None;
        self.push_log(error);
    }

    fn finish_stopped(&mut self, exit_status: ExitStatus) {
        self.pid = None;
        self.status = ServiceStatus::Stopped;
        self.last_error = None;
        self.last_exit_code = exit_status.code();
        self.consecutive_health_failures = 0;
        self.last_poll_at = None;
        self.push_log(format!(
            "ASR service stopped with {}.",
            format_exit_status(exit_status)
        ));
    }

    fn wait_for_startup_health(&mut self, port: u16) {
        let deadline = Instant::now() + STARTUP_HEALTH_TIMEOUT;

        loop {
            self.refresh_child_status();
            if self.child.is_none() || self.status == ServiceStatus::Error {
                return;
            }

            match request_health(port) {
                Ok(health) => {
                    self.status = ServiceStatus::Running;
                    self.last_error = None;
                    self.consecutive_health_failures = 0;
                    self.last_poll_at = Some(Instant::now());
                    self.push_log(format!(
                        "ASR service became healthy{}.",
                        health
                            .version
                            .as_ref()
                            .map(|version| format!(" (version {version})"))
                            .unwrap_or_default()
                    ));
                    self.poll_model_status(port);
                    return;
                }
                Err(error) => {
                    if Instant::now() >= deadline {
                        let message = format!(
                            "ASR service did not become healthy within 15 seconds: {}",
                            error
                        );
                        self.status = ServiceStatus::Error;
                        self.last_error = Some(message.clone());
                        self.push_log(message);
                        return;
                    }
                    thread::sleep(STARTUP_HEALTH_INTERVAL);
                }
            }
        }
    }

    fn poll_running_status_if_due(&mut self) {
        if self.status != ServiceStatus::Running {
            return;
        }

        if let Some(last_poll_at) = self.last_poll_at {
            if last_poll_at.elapsed() < RUNNING_POLL_INTERVAL {
                return;
            }
        }

        let Some(port) = self.port else {
            return;
        };

        self.last_poll_at = Some(Instant::now());
        match request_health(port) {
            Ok(_) => {
                if self.consecutive_health_failures > 0 {
                    self.push_log("ASR health check recovered.");
                }
                self.consecutive_health_failures = 0;
                self.last_error = None;
                self.poll_model_status(port);
            }
            Err(error) => {
                self.consecutive_health_failures =
                    self.consecutive_health_failures.saturating_add(1);
                self.push_log(format!(
                    "ASR health check failed ({}/{}): {error}",
                    self.consecutive_health_failures, MAX_HEALTH_FAILURES
                ));

                if self.consecutive_health_failures >= MAX_HEALTH_FAILURES {
                    let message = format!(
                        "ASR health check failed {MAX_HEALTH_FAILURES} consecutive times: {error}"
                    );
                    self.status = ServiceStatus::Error;
                    self.last_error = Some(message.clone());
                    self.push_log(message);
                }
            }
        }
    }

    fn poll_model_status(&mut self, port: u16) {
        match request_model_status(port) {
            Ok(model_status) => {
                self.model_status = map_model_status(&model_status.status);
                self.resolved_model_id = Some(model_status.model_id);
                if self.model_status == ModelStatus::Error {
                    let message = format!(
                        "ASR model error: {}",
                        model_status
                            .error
                            .unwrap_or_else(|| "unknown model error".to_string())
                    );
                    self.last_error = Some(message.clone());
                    self.push_log(message);
                }
            }
            Err(error) => {
                self.model_status = ModelStatus::Unknown;
                self.push_log(format!("Failed to poll ASR model status: {error}"));
            }
        }
    }

    fn wait_for_model_load(&mut self, port: u16) {
        let deadline = Instant::now() + MODEL_LOAD_POLL_TIMEOUT;

        loop {
            self.poll_model_status(port);
            match self.model_status {
                ModelStatus::Ready => {
                    self.last_error = None;
                    self.push_log("ASR model is ready.");
                    return;
                }
                ModelStatus::Error => return,
                _ => {}
            }

            if Instant::now() >= deadline {
                self.model_status = ModelStatus::Error;
                let message = "ASR model did not become ready within 120 seconds.".to_string();
                self.last_error = Some(message.clone());
                self.push_log(message);
                return;
            }

            thread::sleep(MODEL_LOAD_POLL_INTERVAL);
        }
    }

    fn attach_existing_service(&mut self, port: u16) -> bool {
        match request_existing_asr_health(port) {
            Ok(health) => {
                self.child = None;
                self.status = ServiceStatus::Running;
                self.pid = None;
                self.last_error = None;
                self.last_exit_code = None;
                self.consecutive_health_failures = 0;
                self.last_poll_at = Some(Instant::now());
                self.push_log(format!(
                    "Detected an existing EchoNote ASR service on {}{}; using it without starting a new process.",
                    base_url(port),
                    health
                        .version
                        .as_ref()
                        .map(|version| format!(" (version {version})"))
                        .unwrap_or_default()
                ));
                self.poll_model_status(port);
                true
            }
            Err(error) => {
                self.push_log(format!(
                    "Port {port} is listening but could not be reused as an EchoNote ASR service: {error}"
                ));
                false
            }
        }
    }

    fn stop_external_service_if_available(&mut self) -> bool {
        let Some(port) = self.port else {
            return false;
        };

        if request_existing_asr_health(port).is_err() {
            return false;
        }

        self.status = ServiceStatus::Stopping;
        self.pid = None;
        self.consecutive_health_failures = 0;
        self.last_poll_at = None;

        match request_shutdown(port) {
            Ok(()) => self.push_log(format!(
                "Sent shutdown request to externally running ASR service at {}.",
                base_url(port)
            )),
            Err(error) => {
                self.fail_stop(format!(
                    "Failed to request shutdown for externally running ASR service: {error}"
                ));
                return true;
            }
        }

        if wait_for_local_port_to_close(port, STOP_TIMEOUT) {
            self.status = ServiceStatus::Stopped;
            self.model_status = ModelStatus::Unknown;
            self.last_error = None;
            self.last_exit_code = Some(0);
            self.push_log("Externally running ASR service stopped.");
        } else {
            self.status = ServiceStatus::Error;
            let message =
                "External ASR service did not stop before timeout after shutdown request."
                    .to_string();
            self.last_error = Some(message.clone());
            self.push_log(message);
        }

        true
    }

    fn push_log(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(path) = &self.companion_log_path {
            append_log_line(path, &message);
        }
        self.recent_logs.push(message);
        if self.recent_logs.len() > MAX_RECENT_LOGS {
            let overflow = self.recent_logs.len() - MAX_RECENT_LOGS;
            self.recent_logs.drain(0..overflow);
        }
    }

    fn asr_log_stdio(&self) -> Result<Stdio, String> {
        let Some(path) = &self.asr_log_path else {
            return Ok(Stdio::null());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create ASR log directory: {error}"))?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map(Stdio::from)
            .map_err(|error| format!("Failed to open ASR service log file: {error}"))
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        self.shutdown_on_app_exit();
    }
}

fn backend_cli_arg(backend: Backend) -> &'static str {
    match backend {
        Backend::Fake => "fake",
        Backend::MlxAudio => "mlx-audio",
    }
}

fn base_url(port: u16) -> String {
    format!("http://{ASR_HOST}:{port}")
}

fn expand_tilde(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home_dir) = std::env::var_os("HOME") {
            return PathBuf::from(home_dir).join(rest);
        }
    }

    PathBuf::from(value)
}

fn resolve_service_dir(value: &str) -> Result<PathBuf, String> {
    let path = expand_tilde(value);
    if path.is_absolute() {
        return require_directory(path);
    }

    let current_dir = std::env::current_dir()
        .map_err(|error| format!("Failed to resolve current directory: {error}"))?;
    let mut candidates = vec![current_dir.join(&path)];

    if let Some(parent) = current_dir.parent() {
        candidates.push(parent.join(&path));
        if let Some(grandparent) = parent.parent() {
            candidates.push(grandparent.join(&path));
        }
    }

    for candidate in candidates {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "ASR service path does not exist or is not a directory: {value}"
    ))
}

fn require_directory(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!(
            "ASR service path does not exist or is not a directory: {}",
            path.display()
        ))
    }
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
    service: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelStatusResponse {
    model_id: String,
    status: String,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelLoadResponse {
    model_id: String,
    status: String,
}

struct HttpResponse {
    status_code: u16,
    body: String,
}

fn request_health(port: u16) -> Result<HealthResponse, String> {
    let response = send_local_http_request("GET", "/health", port)?;
    if !(200..300).contains(&response.status_code) {
        return Err(format!(
            "GET /health returned HTTP {}",
            response.status_code
        ));
    }

    let health: HealthResponse = serde_json::from_str(&response.body)
        .map_err(|error| format!("failed to parse /health response: {error}"))?;
    if health.status != "ok" {
        return Err(format!("GET /health returned status '{}'", health.status));
    }
    if health
        .service
        .as_deref()
        .is_some_and(|service| service != "echonote-asr")
    {
        return Err("GET /health returned an unexpected service name".to_string());
    }
    Ok(health)
}

fn request_existing_asr_health(port: u16) -> Result<HealthResponse, String> {
    let health = request_health(port)?;
    match health.service.as_deref() {
        Some("echonote-asr") => Ok(health),
        Some(service) => Err(format!(
            "GET /health returned unexpected service name '{service}'"
        )),
        None => Err("GET /health did not identify an EchoNote ASR service".to_string()),
    }
}

fn local_port_is_listening(port: u16) -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok()
}

fn request_model_status(port: u16) -> Result<ModelStatusResponse, String> {
    let response = send_local_http_request("GET", "/model/status", port)?;
    if !(200..300).contains(&response.status_code) {
        return Err(format!(
            "GET /model/status returned HTTP {}",
            response.status_code
        ));
    }

    serde_json::from_str(&response.body)
        .map_err(|error| format!("failed to parse /model/status response: {error}"))
}

fn request_model_load(port: u16, model_id: &str) -> Result<ModelLoadResponse, String> {
    let body = serde_json::json!({ "model_id": model_id }).to_string();
    let response = send_local_http_request_with_body(
        "POST",
        "/model/load",
        port,
        Some(("application/json", body.as_bytes())),
        LOCAL_MODEL_LOAD_TIMEOUT,
    )?;
    if !(200..300).contains(&response.status_code) {
        return Err(format!(
            "POST /model/load returned HTTP {}: {}",
            response.status_code, response.body
        ));
    }

    serde_json::from_str(&response.body)
        .map_err(|error| format!("failed to parse /model/load response: {error}"))
}

fn request_shutdown(port: u16) -> Result<(), String> {
    let response = send_local_http_request("POST", "/shutdown", port)?;
    if (200..300).contains(&response.status_code) {
        Ok(())
    } else {
        Err(format!(
            "POST /shutdown returned HTTP {}",
            response.status_code
        ))
    }
}

fn send_local_http_request(method: &str, path: &str, port: u16) -> Result<HttpResponse, String> {
    send_local_http_request_with_body(method, path, port, None, LOCAL_HTTP_TIMEOUT)
}

fn send_local_http_request_with_body(
    method: &str,
    path: &str,
    port: u16,
    body: Option<(&str, &[u8])>,
    timeout: Duration,
) -> Result<HttpResponse, String> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| format!("failed to connect to {path}: {error}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("failed to set {path} read timeout: {error}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| format!("failed to set {path} write timeout: {error}"))?;

    let body_length = body.map(|(_, body)| body.len()).unwrap_or(0);
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ASR_HOST}:{port}\r\n{content_type}Content-Length: {body_length}\r\nConnection: close\r\n\r\n",
        content_type = body
            .map(|(content_type, _)| format!("Content-Type: {content_type}\r\n"))
            .unwrap_or_default()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("failed to send {path} request: {error}"))?;
    if let Some((_, body)) = body {
        stream
            .write_all(body)
            .map_err(|error| format!("failed to send {path} request body: {error}"))?;
    }

    let mut raw_response = String::new();
    stream
        .read_to_string(&mut raw_response)
        .map_err(|error| format!("failed to read {path} response: {error}"))?;

    parse_http_response(&raw_response)
}

fn parse_http_response(raw_response: &str) -> Result<HttpResponse, String> {
    let (headers, body) = raw_response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "HTTP response did not contain a header/body separator".to_string())?;
    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| "HTTP response did not contain a status line".to_string())?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("HTTP response status line is invalid: {status_line}"))?
        .parse::<u16>()
        .map_err(|error| format!("failed to parse HTTP response code: {error}"))?;

    Ok(HttpResponse {
        status_code,
        body: body.to_string(),
    })
}

fn map_model_status(status: &str) -> ModelStatus {
    match status {
        "not_loaded" => ModelStatus::NotLoaded,
        "loading" => ModelStatus::Loading,
        "ready" => ModelStatus::Ready,
        "error" => ModelStatus::Error,
        _ => ModelStatus::Unknown,
    }
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<Option<ExitStatus>, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(exit_status)) => return Ok(Some(exit_status)),
            Ok(None) if Instant::now() >= deadline => return Ok(None),
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(format!("Failed to inspect ASR service process: {error}")),
        }
    }
}

fn wait_for_local_port_to_close(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !local_port_is_listening(port) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn format_exit_status(exit_status: ExitStatus) -> String {
    match exit_status.code() {
        Some(code) => format!("exit code {code}"),
        None => "signal termination".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{wait_for_child_exit, ProcessManager, MAX_HEALTH_FAILURES, RUNNING_POLL_INTERVAL};
    use crate::settings::CompanionSettings;
    use crate::state::{ModelStatus, ServiceStatus};
    use std::fs;
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn starts_stops_and_restarts_fake_service_without_terminal() {
        let service_dir = temp_service_dir(SERVER_MODULE);
        let port = unused_port();
        let settings = settings_for(&service_dir, port);
        let mut manager = ProcessManager::default();

        manager.start(&settings);
        assert_eq!(
            manager.status,
            ServiceStatus::Running,
            "{:?}",
            manager.last_error
        );
        assert!(manager.pid.is_some());
        wait_for_port(port);

        let first_pid = manager.pid;
        manager.restart(&settings);
        assert_eq!(
            manager.status,
            ServiceStatus::Running,
            "{:?}",
            manager.last_error
        );
        assert!(manager.pid.is_some());
        assert_ne!(manager.pid, first_pid);
        wait_for_port(port);

        manager.stop();
        assert_eq!(
            manager.status,
            ServiceStatus::Stopped,
            "{:?}",
            manager.last_error
        );
        assert!(manager.child.is_none());
        assert_eq!(manager.last_exit_code, Some(0));

        let _ = fs::remove_dir_all(service_dir);
    }

    #[test]
    fn unexpected_child_exit_moves_state_to_error() {
        let service_dir = temp_service_dir("import sys\nsys.exit(7)\n");
        let settings = settings_for(&service_dir, unused_port());
        let mut manager = ProcessManager::default();

        manager.start(&settings);
        let deadline = Instant::now() + Duration::from_secs(2);
        while manager.status != ServiceStatus::Error && Instant::now() < deadline {
            manager.refresh_child_status();
            thread::sleep(Duration::from_millis(50));
        }

        assert_eq!(manager.status, ServiceStatus::Error);
        assert!(manager.child.is_none());
        assert_eq!(manager.last_exit_code, Some(7));
        assert!(manager
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("exited unexpectedly"));

        let _ = fs::remove_dir_all(service_dir);
    }

    #[test]
    fn start_fails_when_port_is_already_in_use() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().expect("read listener address").port();
        let service_dir = temp_service_dir(SERVER_MODULE);
        let settings = settings_for(&service_dir, port);
        let mut manager = ProcessManager::default();

        manager.start(&settings);

        assert_eq!(manager.status, ServiceStatus::Error);
        assert!(manager.child.is_none());
        assert!(manager
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("already in use"));

        drop(listener);
        let _ = fs::remove_dir_all(service_dir);
    }

    #[test]
    fn start_reuses_existing_echonote_asr_service() {
        let service_dir = temp_service_dir(SERVER_MODULE);
        let port = unused_port();
        let settings = settings_for(&service_dir, port);
        let mut external_child = Command::new(&settings.python_path)
            .current_dir(&service_dir)
            .arg("-m")
            .arg("echonote_asr")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--model")
            .arg(settings.resolved_model_id())
            .arg("--backend")
            .arg("fake")
            .arg("--log-level")
            .arg("info")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start external fake service");
        wait_for_port(port);

        let mut manager = ProcessManager::default();
        manager.start(&settings);

        assert_eq!(
            manager.status,
            ServiceStatus::Running,
            "{:?}",
            manager.last_error
        );
        assert!(manager.child.is_none());
        assert_eq!(manager.pid, None);
        assert_eq!(manager.model_status, ModelStatus::NotLoaded);
        assert!(manager
            .recent_logs
            .iter()
            .any(|line| line.contains("existing EchoNote ASR service")));

        manager.stop();
        assert_eq!(
            manager.status,
            ServiceStatus::Stopped,
            "{:?}",
            manager.last_error
        );
        assert!(matches!(
            wait_for_child_exit(&mut external_child, Duration::from_secs(2)),
            Ok(Some(_))
        ));

        let _ = fs::remove_dir_all(service_dir);
    }

    #[test]
    fn drop_stops_reused_existing_echonote_asr_service() {
        let service_dir = temp_service_dir(SERVER_MODULE);
        let port = unused_port();
        let settings = settings_for(&service_dir, port);
        let mut external_child = Command::new(&settings.python_path)
            .current_dir(&service_dir)
            .arg("-m")
            .arg("echonote_asr")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--model")
            .arg(settings.resolved_model_id())
            .arg("--backend")
            .arg("fake")
            .arg("--log-level")
            .arg("info")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start external fake service");
        wait_for_port(port);

        let mut manager = ProcessManager::default();
        manager.start(&settings);
        assert_eq!(
            manager.status,
            ServiceStatus::Running,
            "{:?}",
            manager.last_error
        );

        drop(manager);

        assert!(matches!(
            wait_for_child_exit(&mut external_child, Duration::from_secs(2)),
            Ok(Some(_))
        ));

        let _ = fs::remove_dir_all(service_dir);
    }

    #[test]
    fn repeated_running_health_failures_move_state_to_error() {
        let service_dir = temp_service_dir(SERVER_MODULE);
        let port = unused_port();
        let settings = settings_for(&service_dir, port);
        let mut manager = ProcessManager::default();

        manager.start(&settings);
        assert_eq!(
            manager.status,
            ServiceStatus::Running,
            "{:?}",
            manager.last_error
        );
        wait_for_port(port);

        let unreachable_port = unused_port();
        manager.port = Some(unreachable_port);
        manager.consecutive_health_failures = MAX_HEALTH_FAILURES - 1;
        manager.last_poll_at = Some(Instant::now() - RUNNING_POLL_INTERVAL);

        let snapshot = manager.snapshot(&settings);
        assert_eq!(snapshot.service_status, ServiceStatus::Error);
        assert!(manager
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("consecutive"));

        manager.port = Some(port);
        manager.stop();
        let _ = fs::remove_dir_all(service_dir);
    }

    fn settings_for(service_dir: &PathBuf, port: u16) -> CompanionSettings {
        CompanionSettings {
            asr_service_path: service_dir.to_string_lossy().into_owned(),
            preferred_port: port,
            ..CompanionSettings::default()
        }
    }

    fn temp_service_dir(module_source: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        let service_dir =
            std::env::temp_dir().join(format!("echonote-companion-process-test-{nonce}"));
        let package_dir = service_dir.join("echonote_asr");
        fs::create_dir_all(&package_dir).expect("create fake service package");
        fs::write(package_dir.join("__init__.py"), "").expect("write fake package init");
        fs::write(package_dir.join("__main__.py"), module_source)
            .expect("write fake service module");
        service_dir
    }

    fn unused_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind random local port");
        listener.local_addr().expect("read local addr").port()
    }

    fn wait_for_port(port: u16) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("fake ASR service did not open port {port}");
    }

    const SERVER_MODULE: &str = r#"
import argparse
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

parser = argparse.ArgumentParser()
parser.add_argument("--host", default="127.0.0.1")
parser.add_argument("--port", type=int, required=True)
parser.add_argument("--model")
parser.add_argument("--backend")
parser.add_argument("--log-level")
args = parser.parse_args()

class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        if self.path == "/health":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"status":"ok","service":"echonote-asr","version":"test"}')
            return
        if self.path == "/model/status":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"model_id":"fake/model","status":"not_loaded","error":null}')
            return
        self.send_response(404)
        self.end_headers()

    def do_POST(self):
        if self.path == "/shutdown":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"status":"shutting_down"}')
            threading.Thread(target=self.server.shutdown, daemon=True).start()
            return
        self.send_response(404)
        self.end_headers()

server = ThreadingHTTPServer((args.host, args.port), Handler)
server.serve_forever()
server.server_close()
"#;
}
