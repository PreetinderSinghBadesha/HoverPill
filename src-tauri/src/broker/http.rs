//! Just enough HTTP/1.1 for one local client: read a single request, write a single response,
//! and notice when the client hangs up while we wait for the user.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const MAX_HEAD: usize = 16 * 1024;
/// `Write` hooks carry the whole file in `tool_input.content`.
const MAX_BODY: usize = 8 * 1024 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

async fn read_more(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Result<(), String> {
    let mut chunk = [0u8; 8192];
    match stream.read(&mut chunk).await {
        Ok(0) => Err("connection closed mid-request".into()),
        Ok(n) => {
            buf.extend_from_slice(&chunk[..n]);
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

pub async fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut buf = Vec::with_capacity(8192);
    let (method, path, headers, head_len) = loop {
        read_more(stream, &mut buf).await?;
        let mut slots = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut slots);
        match req.parse(&buf).map_err(|e| e.to_string())? {
            httparse::Status::Complete(len) => {
                let headers = req
                    .headers
                    .iter()
                    .map(|h| (h.name.to_string(), String::from_utf8_lossy(h.value).into_owned()))
                    .collect::<Vec<_>>();
                break (req.method.unwrap_or("").to_string(), req.path.unwrap_or("").to_string(), headers, len);
            }
            httparse::Status::Partial if buf.len() > MAX_HEAD => return Err("headers too large".into()),
            httparse::Status::Partial => {}
        }
    };

    let mut req = Request { method, path, headers, body: Vec::new() };
    let rest = buf.split_off(head_len);
    if req.header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        req.body = read_chunked(stream, rest).await?;
    } else if let Some(len) = req.header("content-length") {
        let len: usize = len.trim().parse().map_err(|_| "bad content-length")?;
        if len > MAX_BODY {
            return Err("body too large".into());
        }
        let mut body = rest;
        while body.len() < len {
            read_more(stream, &mut body).await?;
        }
        body.truncate(len);
        req.body = body;
    }
    Ok(req)
}

async fn read_chunked(stream: &mut TcpStream, mut raw: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = loop {
            if let Some(i) = raw[pos..].windows(2).position(|w| w == b"\r\n") {
                break pos + i;
            }
            read_more(stream, &mut raw).await?;
        };
        let line = std::str::from_utf8(&raw[pos..line_end]).map_err(|_| "bad chunk header")?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| "bad chunk size")?;
        pos = line_end + 2;
        if size == 0 {
            return Ok(body); // trailers, if any, are ignored
        }
        if body.len() + size > MAX_BODY {
            return Err("body too large".into());
        }
        while raw.len() < pos + size + 2 {
            read_more(stream, &mut raw).await?;
        }
        body.extend_from_slice(&raw[pos..pos + size]);
        pos += size + 2;
    }
}

/// Resolves once the client closes the connection. Claude Code does that when the user
/// interrupts it, or when the hook times out.
pub async fn closed(stream: &mut TcpStream) {
    let mut scratch = [0u8; 512];
    loop {
        match stream.read(&mut scratch).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

pub async fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    stream.shutdown().await
}
