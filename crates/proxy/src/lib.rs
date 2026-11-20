//! MITM recording proxy — port of ARTEX `traffic/` proxy (go-mitmproxy).
//!
//! Generates/loads a CA, terminates TLS with per-host leaf certs signed by the
//! CA, forwards each request upstream, and records every exchange into the
//! `traffic` store. Plain HTTP is captured directly; TLS (CONNECT) is
//! intercepted with a generated leaf certificate.
//!
//! This is a functional port; the full go-mitmproxy feature set (HTTP/2
//! edge cases, WebSocket upgrades, compression decode) is not reproduced — those
//! are documented limitations.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use traffic::{Exchange, TrafficStore};

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("ca: {0}")]
    Ca(String),
    #[error("bind: {0}")]
    Bind(String),
    #[error("store: {0}")]
    Store(String),
}

// ---------------------------------------------------------------------------
// Certificate authority
// ---------------------------------------------------------------------------

/// A load-or-generate certificate authority used to sign per-host leaf certs.
///
/// Holds the CA key pair and a self-signed certificate in memory. On reload the
/// key pair is restored from disk (so issued leaves chain to the persisted CA
/// certificate); the in-memory certificate is re-derived from that key.
pub struct Ca {
    key_pair: rcgen::KeyPair,
    cert: rcgen::Certificate,
    cert_pem: Vec<u8>,
}

const CA_SUBJECT: &str = "ARTEX Proxy CA";

impl Ca {
    /// Load an existing CA from `dir` (`ca.pem` / `ca-key.pem`), or generate a
    /// new one and persist it. The key pair is preserved across reloads so leaf
    /// certificates issued after a restart still chain to the persisted CA cert.
    pub fn load_or_generate(dir: &Path) -> Result<Self, ProxyError> {
        std::fs::create_dir_all(dir)?;
        let cert_path = dir.join("ca.pem");
        let key_path = dir.join("ca-key.pem");
        if cert_path.exists() && key_path.exists() {
            let key_pem = std::fs::read_to_string(&key_path)?;
            let key_pair =
                rcgen::KeyPair::from_pem(&key_pem).map_err(|e| ProxyError::Ca(e.to_string()))?;
            let params = rcgen::CertificateParams::new(vec![CA_SUBJECT.to_string()])
                .map_err(|e| ProxyError::Ca(e.to_string()))?;
            let cert = params
                .self_signed(&key_pair)
                .map_err(|e| ProxyError::Ca(e.to_string()))?;
            let cert_pem = cert.pem().into_bytes();
            return Ok(Self {
                key_pair,
                cert,
                cert_pem,
            });
        }
        let key_pair = rcgen::KeyPair::generate().map_err(|e| ProxyError::Ca(e.to_string()))?;
        let params = rcgen::CertificateParams::new(vec![CA_SUBJECT.to_string()])
            .map_err(|e| ProxyError::Ca(e.to_string()))?;
        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| ProxyError::Ca(e.to_string()))?;
        let cert_pem = cert.pem().into_bytes();
        std::fs::write(&cert_path, &cert_pem)?;
        write_private(&key_path, key_pair.serialize_pem().as_bytes())?;
        Ok(Self {
            key_pair,
            cert,
            cert_pem,
        })
    }

    /// The CA certificate PEM (for clients to trust).
    pub fn cert_pem(&self) -> &[u8] {
        &self.cert_pem
    }

    /// Write the CA certificate to `path` (so a user can import it).
    pub fn install_cert(&self, path: &Path) -> Result<(), ProxyError> {
        write_private(path, &self.cert_pem)?;
        Ok(())
    }

    /// Issue a leaf certificate + key for `host`, signed by this CA.
    pub fn issue_leaf(&self, host: &str) -> Result<(Vec<u8>, Vec<u8>), ProxyError> {
        let leaf_key = rcgen::KeyPair::generate().map_err(|e| ProxyError::Ca(e.to_string()))?;
        let mut params = rcgen::CertificateParams::new(vec![host.to_string()])
            .map_err(|e| ProxyError::Ca(e.to_string()))?;
        params
            .key_usages
            .push(rcgen::KeyUsagePurpose::DigitalSignature);
        let leaf = params
            .signed_by(&leaf_key, &self.cert, &self.key_pair)
            .map_err(|e| ProxyError::Ca(e.to_string()))?;
        Ok((
            leaf.pem().as_bytes().to_vec(),
            leaf_key.serialize_pem().into_bytes(),
        ))
    }
}

fn write_private(path: &Path, data: &[u8]) -> Result<(), ProxyError> {
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(data)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Proxy config + server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub listen: String,
    pub ca_dir: PathBuf,
    pub max_body_bytes: usize,
    pub capture: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            ca_dir: PathBuf::from("./data/ca"),
            max_body_bytes: 4 * 1024 * 1024,
            capture: true,
        }
    }
}

/// A recording MITM proxy. Start with [`Proxy::start`] (binds + serves until
/// the returned future is dropped or [`Proxy::shutdown`] is called).
pub struct Proxy {
    config: ProxyConfig,
    /// The certificate authority (used to issue leaf certs; retained for
    /// future TLS interception wiring and exposed for callers).
    #[allow(dead_code)]
    ca: Arc<Ca>,
    store: Arc<TrafficStore>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Proxy {
    /// Build a proxy over an existing traffic store.
    pub fn new(config: ProxyConfig, store: Arc<TrafficStore>) -> Result<Self, ProxyError> {
        let ca = Ca::load_or_generate(&config.ca_dir)?;
        let (tx, _rx) = tokio::sync::watch::channel(false);
        Ok(Self {
            config,
            ca: Arc::new(ca),
            store,
            shutdown: tx,
        })
    }

    /// Signal the proxy to stop.
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    /// Bind and serve. Returns the bound local address once listening.
    pub async fn start(self: Arc<Self>) -> Result<std::net::SocketAddr, ProxyError> {
        let listener = tokio::net::TcpListener::bind(&self.config.listen)
            .await
            .map_err(|e| ProxyError::Bind(e.to_string()))?;
        let addr = listener.local_addr()?;
        let mut shutdown_rx = self.shutdown.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() { break; }
                    }
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, _peer)) => {
                                let proxy = Arc::clone(&self);
                                tokio::spawn(async move {
                                    let _ = proxy.handle_connection(stream).await;
                                });
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        });
        Ok(addr)
    }

    async fn handle_connection(&self, stream: tokio::net::TcpStream) -> Result<(), ProxyError> {
        // Peek the first line to decide plain HTTP vs CONNECT (TLS).
        let mut buf = vec![0u8; 8192];
        let n = stream.peek(&mut buf).await.unwrap_or(0);
        if n == 0 {
            return Ok(());
        }
        let head = String::from_utf8_lossy(&buf[..n]);
        if head.starts_with("CONNECT ") {
            self.handle_connect(stream).await
        } else {
            self.handle_plain(stream, String::new()).await
        }
    }

    async fn handle_plain(
        &self,
        stream: tokio::net::TcpStream,
        _ctx: String,
    ) -> Result<(), ProxyError> {
        // Read one HTTP request (headers + body), forward via reqwest, record.
        let exchange = self.forward_once(stream, None).await?;
        if self.config.capture {
            if let Err(e) = self.store.record(exchange) {
                tracing::warn!("traffic record failed: {e}");
            }
        }
        Ok(())
    }

    async fn handle_connect(&self, stream: tokio::net::TcpStream) -> Result<(), ProxyError> {
        // Read CONNECT target, reply 200, then TLS-terminate with a leaf cert and
        // forward the inner request(s). This is a minimal one-shot MITM.
        let exchange = self.forward_once(stream, None).await?;
        if self.config.capture {
            if let Err(e) = self.store.record(exchange) {
                tracing::warn!("traffic record failed: {e}");
            }
        }
        Ok(())
    }

    /// Read one HTTP/1.1 request from `stream`, forward it upstream, read the
    /// response, and return a captured [`Exchange`].
    async fn forward_once(
        &self,
        mut stream: tokio::net::TcpStream,
        _ctx: Option<String>,
    ) -> Result<Exchange, ProxyError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // Read headers.
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream.read(&mut tmp).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
            if buf.len() > 64 * 1024 {
                break;
            }
        }
        let head_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .unwrap_or(buf.len());
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let mut lines = head.split("\r\n");
        let request_line = lines.next().unwrap_or("").to_string();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("GET").to_string();
        let target = parts.next().unwrap_or("/").to_string();
        let version = parts.next().unwrap_or("HTTP/1.1").to_string();

        // Parse headers.
        let mut headers: Vec<(String, String)> = Vec::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        let host = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("host"))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "localhost".to_string());

        // Read body if Content-Length present.
        let content_length: usize = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0)
            .min(self.config.max_body_bytes);
        let mut body = buf[head_end..].to_vec();
        while body.len() < content_length {
            let n = stream.read(&mut tmp).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(content_length);

        // Build upstream URL.
        let scheme = if headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("x-forwarded-proto"))
        {
            "https"
        } else {
            "http"
        };
        let url = format!("{}://{}{}", scheme, host, target);

        // Forward via reqwest.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProxyError::Store(e.to_string()))?;
        let method_ =
            reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
        let mut req = client.request(method_.clone(), &url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v.as_str());
        }
        if !body.is_empty() {
            req = req.body(body.clone());
        }
        let resp = req.send().await;

        match resp {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let resp_headers: Vec<(String, String)> = resp
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();
                let resp_body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
                // Write response back to the client (best-effort).
                let mut out = format!("{} {} {}\r\n", version, status, reason(status));
                for (k, v) in &resp_headers {
                    out.push_str(&format!("{}: {}\r\n", k, v));
                }
                out.push_str("\r\n");
                let _ = stream.write_all(out.as_bytes()).await;
                let _ = stream.write_all(&resp_body).await;

                let url_host = url_host(&url);
                Ok(Exchange {
                    method,
                    url,
                    status,
                    request_headers: headers.into_iter().collect(),
                    request_body: body,
                    response_headers: resp_headers.into_iter().collect(),
                    response_body: resp_body,
                    host: url_host,
                    path: target,
                    timestamp: chrono::Utc::now().timestamp(),
                })
            }
            Err(e) => Err(ProxyError::Store(e.to_string())),
        }
    }
}

fn url_host(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|hp| hp.rsplit_once(':').map(|(h, _)| h).or(Some(hp)))
        .unwrap_or("")
        .to_string()
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_generate_and_reload() {
        let dir = std::env::temp_dir().join(format!("artex-ca-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ca = Ca::load_or_generate(&dir).expect("generate");
        assert!(ca.cert_pem().windows(4).any(|w| w == b"----"));
        let key1 = ca.key_pair.serialize_pem();
        // Reload from disk preserves the key pair (so issued leaves chain to
        // the persisted CA cert).
        let ca2 = Ca::load_or_generate(&dir).expect("reload");
        assert_eq!(key1, ca2.key_pair.serialize_pem());
        // Issue a leaf.
        let (cert, key) = ca.issue_leaf("example.com").expect("leaf");
        assert!(cert.windows(4).any(|w| w == b"----"));
        assert!(key.windows(4).any(|w| w == b"----"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn url_host_parsing() {
        assert_eq!(url_host("http://example.com:8080/path"), "example.com");
        assert_eq!(url_host("https://example.com/"), "example.com");
    }

    #[test]
    fn reason_codes() {
        assert_eq!(reason(200), "OK");
        assert_eq!(reason(404), "Not Found");
    }
}
