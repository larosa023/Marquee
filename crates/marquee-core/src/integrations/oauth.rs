//! Helpers for browser sign-ins: PKCE and a one-shot loopback listener that catches the redirect.

use crate::error::{Error, Result};
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The loopback port Marquee listens on during a sign-in. Redirect URIs registered with
/// Spotify must use exactly this port.
pub const PORT: u16 = 43117;

pub fn random_string(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

/// Returns (verifier, challenge) for PKCE with S256.
pub fn pkce() -> (String, String) {
    let verifier = random_string(64);
    (verifier.clone(), challenge(&verifier))
}

pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

const DONE_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Marquee</title>
<style>html{background:#0a0a0c;color:#f4f1ea;font:16px/1.5 "Segoe UI Variable","Segoe UI",system-ui,sans-serif}
body{display:grid;place-items:center;height:100vh;margin:0}main{text-align:center}
h1{font-size:28px;margin:0 0 8px;color:#f2c14e}p{margin:0;opacity:.75}</style></head>
<body><main><h1>__TITLE__</h1><p>__BODY__</p></main></body></html>"#;

pub fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            let dec = |x: &str| urlencoding::decode(&x.replace('+', " ")).map(|c| c.into_owned()).unwrap_or_else(|_| x.to_string());
            (dec(k), dec(v))
        })
        .collect()
}

/// Waits for the browser to land on `http://127.0.0.1:<port><path>?...` and returns the query
/// parameters. Other requests (favicons and the like) get a 404 and are ignored.
pub async fn wait_for_redirect(port: u16, path: &str, timeout: Duration) -> Result<HashMap<String, String>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        Error::msg(format!("Couldn't listen on 127.0.0.1:{port} for the sign-in ({e}). Close whatever is using that port and try again."))
    })?;
    let accept = async {
        loop {
            let (mut sock, _) = listener.accept().await?;
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("");
            let (req_path, query) = target.split_once('?').unwrap_or((target, ""));
            if req_path != path {
                let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            }
            let params = parse_query(query);
            let ok = !params.contains_key("error");
            let page = DONE_PAGE
                .replace("__TITLE__", if ok { "You're signed in" } else { "Sign-in was cancelled" })
                .replace("__BODY__", if ok { "You can close this tab and go back to Marquee." } else { "Nothing was connected. You can close this tab." });
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", page.len(), page);
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
            return Ok::<_, std::io::Error>(params);
        }
    };
    match tokio::time::timeout(timeout, accept).await {
        Ok(Ok(params)) => Ok(params),
        Ok(Err(e)) => Err(Error::Io(e)),
        Err(_) => Err(Error::msg("The sign-in wasn't finished in time. Try connecting again.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc_7636_example() {
        // Appendix B of RFC 7636.
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let (v, c) = pkce();
        assert_eq!(v.len(), 64);
        assert_eq!(challenge(&v), c);
    }

    #[test]
    fn query_strings_decode() {
        let q = parse_query("code=abc%2F123&state=x+y&empty=");
        assert_eq!(q["code"], "abc/123");
        assert_eq!(q["state"], "x y");
        assert_eq!(q["empty"], "");
    }

    #[tokio::test]
    async fn the_listener_returns_the_redirect_and_ignores_other_paths() {
        let port = 43990 + (std::process::id() % 500) as u16;
        let wait = tokio::spawn(wait_for_redirect(port, "/spotify", Duration::from_secs(5)));
        tokio::time::sleep(Duration::from_millis(150)).await;
        for target in ["/favicon.ico", "/spotify?code=the-code&state=s1"] {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).await.unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out).await;
        }
        let params = wait.await.unwrap().unwrap();
        assert_eq!(params["code"], "the-code");
        assert_eq!(params["state"], "s1");
    }
}
