//! Loopback HTTP mock shared by the provider fixture corpora: canned
//! responses in, captured requests out, one request per connection. Each
//! dialect passes its own base path to `MockServer::start`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// One served HTTP response; the mock always closes the connection after
/// writing it.
pub struct CannedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl CannedResponse {
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![(
                String::from("content-type"),
                String::from("application/json"),
            )],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn sse(body: &[u8]) -> Self {
        Self {
            status: 200,
            headers: vec![(
                String::from("content-type"),
                String::from("text/event-stream"),
            )],
            body: body.to_vec(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((String::from(name), String::from(value)));
        self
    }
}

/// One captured request, parsed off the raw bytes the adapter sent.
pub struct CapturedRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

pub struct MockServer {
    pub base_url: String,
    log: Arc<Mutex<Vec<String>>>,
    handle: JoinHandle<()>,
}

impl MockServer {
    /// Binds loopback and serves `responses` in order at `base_path`;
    /// requests beyond the list repeat the last canned response.
    pub async fn start(responses: Vec<CannedResponse>, base_path: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let responses = Arc::new(responses);
        let handle = tokio::spawn(async move {
            let mut served = 0usize;
            while let Ok((socket, _)) = listener.accept().await {
                let responses = responses.clone();
                let sink = sink.clone();
                let index = served;
                served = served.saturating_add(1);
                tokio::spawn(serve_connection(socket, responses, sink, index));
            }
        });
        Ok(Self {
            base_url: format!("http://{addr}{base_path}"),
            log,
            handle,
        })
    }

    pub fn request_count(&self) -> usize {
        self.log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn request(&self, index: usize) -> Option<CapturedRequest> {
        let log = self
            .log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Some(parse_request(log.get(index)?))
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn serve_connection(
    mut socket: TcpStream,
    responses: Arc<Vec<CannedResponse>>,
    sink: Arc<Mutex<Vec<String>>>,
    index: usize,
) {
    let raw = read_request(&mut socket).await;
    sink.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(String::from_utf8_lossy(&raw).into_owned());
    let response = match responses.get(index).or_else(|| responses.last()) {
        Some(response) => response,
        None => return,
    };
    let _ = write_response(&mut socket, response).await;
    let _ = socket.shutdown().await;
}

async fn read_request(socket: &mut TcpStream) -> Vec<u8> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                raw.extend_from_slice(&chunk[..read]);
                if request_complete(&raw) || raw.len() >= MAX_REQUEST_BYTES {
                    break;
                }
            }
        }
    }
    raw
}

/// True once the request head and the content-length body have arrived.
fn request_complete(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(raw);
    let Some(head_end) = text.find("\r\n\r\n") else {
        return false;
    };
    let head = text[..head_end].to_ascii_lowercase();
    let Some(at) = head.find("content-length:") else {
        return false;
    };
    let rest = &head[at + "content-length:".len()..];
    let digits: String = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    let length = digits.parse::<usize>().unwrap_or(0);
    text.len() >= head_end + 4 + length
}

async fn write_response(socket: &mut TcpStream, response: &CannedResponse) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} x\r\n", response.status);
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        response.body.len()
    ));
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(&response.body).await?;
    socket.flush().await
}

fn parse_request(raw: &str) -> CapturedRequest {
    let (head, body) = match raw.split_once("\r\n\r\n") {
        Some((head, body)) => (head, body),
        None => (raw, ""),
    };
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    CapturedRequest {
        method,
        path,
        headers,
        body: String::from(body),
    }
}
