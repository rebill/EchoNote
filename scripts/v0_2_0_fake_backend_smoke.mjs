#!/usr/bin/env node
import { spawn } from "child_process";
import { createRequire } from "module";
import { mkdtemp, rm, writeFile } from "fs/promises";
import { existsSync } from "fs";
import net from "net";
import { tmpdir } from "os";
import { dirname, join, resolve } from "path";
import { fileURLToPath } from "url";

const DEFAULT_MODEL_ID = "mlx-community/Qwen3-ASR-0.6B-4bit";
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const asrServiceDir = join(repoRoot, "asr-service");
const pluginDir = join(repoRoot, "plugin");
const tempDir = await mkdtemp(join(tmpdir(), "echonote-v0-2-fake-smoke-"));
const discoveryPath = join(tempDir, "companion.json");
const runnerPath = join(tempDir, "plugin-runtime-runner.mjs");

let asrProcess;
let asrExit = null;
let asrStdout = "";
let asrStderr = "";
let asrBaseUrl = null;

try {
  const pythonPath = resolvePythonPath();
  const port = await getOpenPort();
  const baseUrl = `http://127.0.0.1:${port}`;
  asrBaseUrl = baseUrl;

  log(`Starting fake ASR service on ${baseUrl}`);
  asrProcess = spawn(
    pythonPath,
    [
      "-m",
      "echonote_asr",
      "--host",
      "127.0.0.1",
      "--port",
      String(port),
      "--model",
      DEFAULT_MODEL_ID,
      "--backend",
      "fake",
      "--log-level",
      "info"
    ],
    {
      cwd: asrServiceDir,
      env: { ...process.env, PYTHONUNBUFFERED: "1" },
      stdio: ["ignore", "pipe", "pipe"]
    }
  );

  asrProcess.stdout.on("data", (chunk) => {
    asrStdout = appendBounded(asrStdout, chunk.toString());
  });
  asrProcess.stderr.on("data", (chunk) => {
    asrStderr = appendBounded(asrStderr, chunk.toString());
  });
  asrProcess.on("exit", (code, signal) => {
    asrExit = { code, signal };
  });

  await waitForHealth(baseUrl);
  log("ASR health is ok");

  await loadModel(baseUrl);
  const modelStatus = await waitForModelReady(baseUrl);
  log(`Model status is ${modelStatus.status}`);

  const transcript = await transcribeFakeChunk(baseUrl);
  if (!transcript.text.includes("fake transcript for chunk smoke")) {
    throw new Error(`Unexpected fake transcript text: ${transcript.text}`);
  }
  log("Fake transcription endpoint returned expected transcript text");

  await writeDiscovery(discoveryPath, {
    version: 1,
    app: "EchoNote ASR Companion",
    service: "echonote-asr",
    status: "running",
    baseUrl,
    host: "127.0.0.1",
    port,
    backend: "fake",
    modelId: DEFAULT_MODEL_ID,
    modelStatus: modelStatus.status,
    pid: asrProcess.pid ?? null,
    updatedAt: new Date().toISOString()
  });
  log(`Wrote temporary discovery file: ${discoveryPath}`);

  await verifyPluginRuntimeResolver(discoveryPath, baseUrl);
  log("Plugin Companion-only runtime resolution checks passed");

  console.log("PASS v0.2.0 fake-backend smoke test");
} finally {
  await shutdownAsr();
  await rm(tempDir, { recursive: true, force: true });
}

function resolvePythonPath() {
  if (process.env.ECHONOTE_ASR_PYTHON) {
    return process.env.ECHONOTE_ASR_PYTHON;
  }

  const venvPython = join(asrServiceDir, ".venv", "bin", "python");
  return existsSync(venvPython) ? venvPython : "python3";
}

async function getOpenPort() {
  return new Promise((resolvePort, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("Failed to allocate local TCP port.")));
        return;
      }
      const { port } = address;
      server.close(() => resolvePort(port));
    });
  });
}

async function waitForHealth(baseUrl) {
  const deadline = Date.now() + 15_000;
  let lastError = null;

  while (Date.now() < deadline) {
    assertAsrStillRunning();
    try {
      const health = await getJson(`${baseUrl}/health`);
      if (health.status === "ok") {
        return;
      }
      lastError = new Error(`Health returned ${JSON.stringify(health)}`);
    } catch (error) {
      lastError = error;
    }
    await sleep(250);
  }

  throw new Error(`ASR service did not become healthy: ${lastError?.message ?? "unknown error"}`);
}

async function loadModel(baseUrl) {
  const response = await fetch(`${baseUrl}/model/load`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ model_id: DEFAULT_MODEL_ID })
  });
  if (!response.ok) {
    throw new Error(`Model load failed: ${await response.text()}`);
  }
}

async function waitForModelReady(baseUrl) {
  const deadline = Date.now() + 10_000;
  let latestStatus = null;

  while (Date.now() < deadline) {
    assertAsrStillRunning();
    latestStatus = await getJson(`${baseUrl}/model/status`);
    if (latestStatus.status === "ready") {
      return latestStatus;
    }
    if (latestStatus.status === "error") {
      throw new Error(`Model entered error state: ${latestStatus.error ?? "unknown error"}`);
    }
    await sleep(250);
  }

  throw new Error(`Model did not become ready: ${JSON.stringify(latestStatus)}`);
}

async function transcribeFakeChunk(baseUrl) {
  const formData = new FormData();
  formData.append("audio", new Blob([createSilentWav()]), "smoke.wav");
  formData.append("chunk_id", "smoke");
  formData.append("started_at_ms", "0");
  formData.append("ended_at_ms", "100");
  formData.append("language", "auto");

  const response = await fetch(`${baseUrl}/transcribe`, {
    method: "POST",
    body: formData
  });
  if (!response.ok) {
    throw new Error(`Fake transcribe failed: ${await response.text()}`);
  }
  return response.json();
}

function createSilentWav() {
  const sampleRate = 16_000;
  const durationMs = 100;
  const channels = 1;
  const bitsPerSample = 16;
  const samples = Math.floor((sampleRate * durationMs) / 1000);
  const dataSize = samples * channels * (bitsPerSample / 8);
  const buffer = Buffer.alloc(44 + dataSize);

  buffer.write("RIFF", 0);
  buffer.writeUInt32LE(36 + dataSize, 4);
  buffer.write("WAVE", 8);
  buffer.write("fmt ", 12);
  buffer.writeUInt32LE(16, 16);
  buffer.writeUInt16LE(1, 20);
  buffer.writeUInt16LE(channels, 22);
  buffer.writeUInt32LE(sampleRate, 24);
  buffer.writeUInt32LE(sampleRate * channels * (bitsPerSample / 8), 28);
  buffer.writeUInt16LE(channels * (bitsPerSample / 8), 32);
  buffer.writeUInt16LE(bitsPerSample, 34);
  buffer.write("data", 36);
  buffer.writeUInt32LE(dataSize, 40);
  return buffer;
}

async function writeDiscovery(path, discovery) {
  await writeFile(path, `${JSON.stringify(discovery, null, 2)}\n`, "utf8");
}

async function verifyPluginRuntimeResolver(path, expectedBaseUrl) {
  const require = createRequire(import.meta.url);
  const esbuild = require(join(pluginDir, "node_modules", "esbuild"));
  const runnerSource = `
    import { DEFAULT_SETTINGS } from ${JSON.stringify(join(pluginDir, "src", "settings", "settings.ts"))};
    import { resolveCompanionDiscoveryFile } from ${JSON.stringify(join(pluginDir, "src", "asr", "companion-discovery.ts"))};
    import { resolveAsrRuntime } from ${JSON.stringify(join(pluginDir, "src", "asr", "asr-runtime-resolver.ts"))};

    const discoveryPath = process.argv[2];
    const expectedBaseUrl = process.argv[3];
    const resolution = await resolveCompanionDiscoveryFile({
      discoveryPath,
      maxAgeSeconds: 30
    });
    if (resolution.kind !== "available") {
      throw new Error("Expected available Companion discovery, got " + JSON.stringify(resolution));
    }
    if (resolution.baseUrl !== expectedBaseUrl) {
      throw new Error("Unexpected Companion baseUrl: " + resolution.baseUrl);
    }

    const runtime = await resolveAsrRuntime({
      ...DEFAULT_SETTINGS,
      asrRuntimeMode: "companion",
      companionDiscoveryPath: discoveryPath,
      companionDiscoveryMaxAgeSeconds: 30
    });
    if (runtime.mode !== "companion") {
      throw new Error("Expected Companion runtime, got " + JSON.stringify(runtime));
    }
    if (runtime.baseUrl !== expectedBaseUrl) {
      throw new Error("Unexpected runtime baseUrl: " + runtime.baseUrl);
    }

    let missingCompanionError = null;
    try {
      await resolveAsrRuntime({
        ...DEFAULT_SETTINGS,
        asrRuntimeMode: "companion",
        companionDiscoveryPath: "/tmp/echonote-missing-companion.json",
        companionDiscoveryMaxAgeSeconds: 30
      });
    } catch (error) {
      missingCompanionError = error;
    }

    if (!missingCompanionError) {
      throw new Error("Expected missing Companion discovery to fail.");
    }
    if (missingCompanionError.code !== "ASR_COMPANION_UNAVAILABLE") {
      throw new Error("Unexpected missing Companion error code: " + missingCompanionError.code);
    }

    const legacyManualRuntime = await resolveAsrRuntime({
      ...DEFAULT_SETTINGS,
      asrRuntimeMode: "manual",
      companionDiscoveryPath: "/tmp/echonote-missing-companion.json",
      companionDiscoveryMaxAgeSeconds: 30
    }).catch((error) => error);
    if (legacyManualRuntime.mode === "manual") {
      throw new Error("Legacy Manual settings must not resolve to plugin-managed ASR.");
    }

    console.log(JSON.stringify({
      discovery: resolution.kind,
      runtime: runtime.mode,
      missingCompanionError: missingCompanionError.code,
      baseUrl: runtime.baseUrl
    }));
  `;

  await esbuild.build({
    stdin: {
      contents: runnerSource,
      resolveDir: repoRoot,
      sourcefile: "plugin-runtime-runner.ts",
      loader: "ts"
    },
    outfile: runnerPath,
    bundle: true,
    format: "esm",
    platform: "node",
    target: "node20",
    logLevel: "silent"
  });

  const result = await runCommand(process.execPath, [runnerPath, path, expectedBaseUrl], {
    cwd: repoRoot
  });
  if (result.code !== 0) {
    throw new Error(`Plugin runtime resolver check failed:\n${result.stdout}\n${result.stderr}`);
  }
}

async function getJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${url} returned ${response.status}: ${await response.text()}`);
  }
  return response.json();
}

async function shutdownAsr() {
  if (!asrProcess || asrExit) {
    return;
  }

  try {
    await fetch(`${asrBaseUrl}/shutdown`, { method: "POST" });
  } catch {
    // The process may exit before the shutdown response is observable.
  }

  const exited = await waitForProcessExit(asrProcess, 3_000);
  if (!exited) {
    asrProcess.kill("SIGTERM");
    await waitForProcessExit(asrProcess, 2_000);
  }
}

function assertAsrStillRunning() {
  if (asrExit) {
    throw new Error(
      `ASR service exited early with code ${asrExit.code} signal ${asrExit.signal}\nstdout:\n${asrStdout}\nstderr:\n${asrStderr}`
    );
  }
}

function runCommand(command, args, options) {
  return new Promise((resolveResult) => {
    const child = spawn(command, args, {
      ...options,
      stdio: ["ignore", "pipe", "pipe"]
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("close", (code) => {
      resolveResult({ code, stdout, stderr });
    });
  });
}

function waitForProcessExit(child, timeoutMs) {
  return new Promise((resolveExited) => {
    if (asrExit) {
      resolveExited(true);
      return;
    }
    const timeout = setTimeout(() => {
      child.off("exit", onExit);
      resolveExited(false);
    }, timeoutMs);
    function onExit() {
      clearTimeout(timeout);
      resolveExited(true);
    }
    child.once("exit", onExit);
  });
}

function appendBounded(current, next) {
  const combined = current + next;
  return combined.length > 8_000 ? combined.slice(-8_000) : combined;
}

function sleep(ms) {
  return new Promise((resolveSleep) => setTimeout(resolveSleep, ms));
}

function log(message) {
  console.log(`[fake-backend-smoke] ${message}`);
}
