//! A scripted HTTP/1.1 server for contract tests: each connection gets the next reply, which
//! can be sent in timed pieces and closed early (a partial stream).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    /// Body pieces, each sent after its delay.
    pub chunks: Vec<(Duration, Vec<u8>)>,
}

impl Reply {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("content-type", content_type.to_owned())],
            chunks: vec![(Duration::ZERO, body.into())],
        }
    }

    pub fn json(status: u16, body: &str) -> Self {
        Self::new(status, "application/json", body)
    }

    pub fn events(pieces: &[&str]) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type", "text/event-stream".to_owned())],
            chunks: pieces
                .iter()
                .map(|piece| (Duration::ZERO, piece.as_bytes().to_vec()))
                .collect(),
        }
    }

    pub fn header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_owned()));
        self
    }

    /// Wait `delay` before each piece.
    pub fn paced(mut self, delay: Duration) -> Self {
        for chunk in &mut self.chunks {
            chunk.0 = delay;
        }
        self
    }
}

#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub path: String,
    /// Lowercased names.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("JSON request body")
    }
}

pub struct Server {
    pub addr: SocketAddr,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    pub fn base(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// Serve `replies` in order on 127.0.0.1; past the end every request gets a 500. Call
/// inside a Tokio runtime.
pub async fn serve(replies: Vec<Reply>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(Mutex::new(VecDeque::from(replies)));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            let queue = queue.clone();
            tokio::spawn(async move {
                let Some(request) = read_request(&mut socket).await else {
                    return;
                };
                log.lock().unwrap().push(request);
                let reply = queue
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| Reply::json(500, r#"{"error":"no scripted reply"}"#));
                let mut head = format!(
                    "HTTP/1.1 {} Scripted\r\nconnection: close\r\n",
                    reply.status
                );
                for (name, value) in &reply.headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                if socket.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                for (delay, chunk) in reply.chunks {
                    tokio::time::sleep(delay).await;
                    if socket.write_all(&chunk).await.is_err() || socket.flush().await.is_err() {
                        return;
                    }
                }
                let _ = socket.shutdown().await;
            });
        }
    });
    Server { addr, seen }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    let head_end = loop {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(at) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    while data.len() < head_end + length {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..read]);
    }
    let body = String::from_utf8_lossy(&data[head_end..]).into_owned();
    Some(Seen {
        method,
        path,
        headers,
        body,
    })
}
