use std::process::{Command, Stdio};

use anyhow::{Context, Result, ensure};

use super::{Cancellation, process::run_conditional};

pub(super) struct Response {
    pub status: u16,
    pub etag: Option<String>,
    pub next: bool,
    pub body: Vec<u8>,
}

pub(super) fn get(endpoint: &str, etag: Option<&str>, cancel: &Cancellation) -> Result<Response> {
    let mut command = Command::new("gh");
    command
        .args([
            "api",
            "--hostname",
            "github.com",
            "--method",
            "GET",
            "--include",
            endpoint,
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(etag) = etag {
        ensure!(!etag.contains(['\r', '\n']), "invalid cached ETag");
        command.args(["--header", &format!("If-None-Match: {etag}")]);
    }
    parse(&run_conditional(&mut command, endpoint, cancel)?)
}

pub(super) fn status(bytes: &[u8]) -> Option<u16> {
    let line = std::str::from_utf8(bytes.split(|b| *b == b'\n').next()?).ok()?;
    let mut words = line.split_whitespace();
    words.next()?.starts_with("HTTP/").then_some(())?;
    words.next()?.parse().ok()
}

fn parse(bytes: &[u8]) -> Result<Response> {
    let status = status(bytes).context("missing HTTP response status")?;
    ensure!(
        matches!(status, 200 | 304),
        "unexpected HTTP status {status}"
    );
    let (end, separator) = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .map(|end| (end, 4))
        .or_else(|| {
            bytes
                .windows(2)
                .position(|part| part == b"\n\n")
                .map(|end| (end, 2))
        })
        .context("missing HTTP response headers")?;
    let headers = std::str::from_utf8(&bytes[..end])?;
    let header = |name: &str| {
        headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    };
    Ok(Response {
        status,
        etag: header("etag").map(str::to_owned),
        next: header("link")
            .is_some_and(|value| value.split(',').any(|link| link.contains("rel=\"next\""))),
        body: bytes[end + separator..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_conditional_and_paginated_responses() {
        let response = parse(b"HTTP/2.0 200 OK\r\nETag: W/\"abc\"\r\nLink: <https://api.github.com/a?page=2>; rel=\"next\"\r\n\r\n[]").unwrap();
        assert_eq!(response.etag.as_deref(), Some("W/\"abc\""));
        assert!(response.next);
        assert_eq!(response.body, b"[]");
        let response = parse(b"HTTP/2.0 304 Not Modified\r\n\r\n").unwrap();
        assert_eq!(response.status, 304);
        assert!(response.body.is_empty());
        assert!(parse(b"HTTP/2.0 403 Forbidden\r\n\r\n{}").is_err());
    }
}
