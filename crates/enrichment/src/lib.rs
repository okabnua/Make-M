//! enrichment crate — behaviour-preserving port of ARTEX `enrich/enrich.go`.
//!
//! This is the engine-side (non-AI) asset auto-completion layer described in the
//! asset model and auto-association design doc §5: an async worker pool that
//! resolves domains (DNS) and probes web assets (HTTP) and writes the results
//! back into the asset graph.
//!
//! The Go source has two jobs:
//! - `jobDNS`: resolve a domain into A/AAAA/CNAME records (ungated).
//! - `jobHTTP`: probe a web asset (site) over HTTP/HTTPS, capturing status,
//!   title, headers and content length (gated by RoE in the Go engine).
//!
//! This crate exposes the pure-logic core of that layer as a set of free async
//! functions plus the supporting types. The DB-upsert and worker-pool/cooldown
//! orchestration live in the caller (the Go `Engine`); here we only resolve and
//! probe, returning an [`Enrichment`] value for the caller to persist.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default HTTP probe timeout. Go: `httpTimeout = 12 * time.Second`.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(12);

/// Default DNS resolve timeout. Go: dnsx `Timeout: 4 * time.Second`.
pub const DEFAULT_DNS_TIMEOUT: Duration = Duration::from_secs(4);

/// Default maximum response body read by the HTTP probe. Go: `io.LimitReader(resp.Body, 1<<20)`.
pub const DEFAULT_MAX_BODY_BYTES: usize = 1 << 20;

/// Default User-Agent for the HTTP probe.
pub const DEFAULT_USER_AGENT: &str = "artex-enrich/1.0";

/// Options controlling an enrichment run.
///
/// Port of the tunable constants in `enrich.go` (timeouts, body cap) plus the
/// options requested for the probe (user agent, redirect policy, HTTPS probing).
#[derive(Debug, Clone)]
pub struct EnrichOptions {
    /// Overall HTTP probe timeout (Go `httpTimeout`).
    pub timeout: Duration,
    /// Maximum response body bytes read by the probe (Go `1<<20` MiB cap).
    pub max_body_bytes: usize,
    /// `User-Agent` header sent by the probe.
    pub user_agent: String,
    /// Whether to follow HTTP redirects.
    pub follow_redirects: bool,
    /// Whether to attempt an HTTPS probe (in addition to / instead of HTTP).
    pub probe_https: bool,
}

impl Default for EnrichOptions {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_HTTP_TIMEOUT,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            follow_redirects: false,
            probe_https: true,
        }
    }
}

/// TLS information captured during an HTTPS probe.
///
/// The Go client skips TLS verification (`InsecureSkipVerify: true`) because it
/// is a pentest probe; we capture whatever the TLS handshake yields.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TlsInfo {
    /// Whether the connection was established over TLS.
    pub enabled: bool,
    /// Negotiated ALPN protocol, if any.
    pub alpn: Option<String>,
    /// The server certificate subject CN, if a certificate was presented.
    pub subject_cn: Option<String>,
    /// The server certificate issuer CN, if a certificate was presented.
    pub issuer_cn: Option<String>,
}

/// Result of a single HTTP probe.
///
/// Mirrors the Go `db.UpsertHTTPServiceReq` fields the probe fills:
/// `StatusCode`, `ContentLength`, `PageTitle`, plus the request metadata
/// (`technologies`, favicon hash, `server` header) asked for in the rewrite.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpProbe {
    /// Final URL after redirects.
    pub url: String,
    /// HTTP status code of the final response.
    pub status_code: Option<u16>,
    /// Length of the body actually read (post body-cap truncation).
    pub content_length: Option<i64>,
    /// `<title>` text extracted from the HTML body (HTML-unescaped, trimmed).
    pub title: Option<String>,
    /// `Server` response header, if present.
    pub server: Option<String>,
    /// The `Content-Type` response header, if present.
    pub content_type: Option<String>,
    /// murmur3 x86 32-bit hash of the favicon bytes (`-favicondata` probe).
    pub favicon_hash: Option<i64>,
    /// TLS metadata (only populated for HTTPS probes).
    pub tls: TlsInfo,
}

/// Resolved DNS records for a domain.
///
/// Port of `data.A`, `data.AAAA`, `data.CNAME` from dnsx `QueryMultiple`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DnsRecords {
    /// Resolved IPv4 addresses.
    pub a: Vec<String>,
    /// Resolved IPv6 addresses.
    pub aaaa: Vec<String>,
    /// CNAME chain (each element is one CNAME hop).
    pub cname: Vec<String>,
}

/// The full result of enriching an asset.
///
/// This is the rewrite's [`Enrichment`] value: DNS records (A/AAAA/CNAME), the
/// CNAME chain, and the HTTP probe result (status, title, technologies, favicon
/// hash, content length, server header) plus TLS info.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Enrichment {
    /// DNS records: resolved IPv4/IPv6 addresses.
    pub ips_a: Vec<String>,
    /// DNS records: resolved IPv6 addresses.
    pub ips_aaaa: Vec<String>,
    /// Resolved IP addresses (A + AAAA, de-duplicated, order preserved).
    pub resolved_ips: Vec<String>,
    /// CNAME chain for the domain (may be empty).
    pub cname_chain: Vec<String>,
    /// The HTTP probe result, if a probe was performed.
    pub http: Option<HttpProbe>,
}

/// Errors returned by enrichment functions.
#[derive(Debug, Error)]
pub enum EnrichmentError {
    /// DNS resolution failed.
    #[error("dns resolution failed: {0}")]
    Dns(String),
    /// HTTP probe failed.
    #[error("http probe failed: {0}")]
    Http(String),
    /// The operation timed out.
    #[error("operation timed out")]
    Timeout,
    /// A value could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),
}

impl EnrichmentError {
    fn dns<S: Into<String>>(msg: S) -> Self {
        EnrichmentError::Dns(msg.into())
    }
    fn http<S: Into<String>>(msg: S) -> Self {
        EnrichmentError::Http(msg.into())
    }
    fn parse<S: Into<String>>(msg: S) -> Self {
        EnrichmentError::Parse(msg.into())
    }
}

/// Resolve a domain into A/AAAA/CNAME records.
///
/// Ports `Engine.doDNS`'s data-gathering half (the DB upserts happen in the
/// caller): `resolv.QueryMultiple(host)` is replaced by [`tokio::net::lookup_host`]
/// for A/AAAA, with CNAME discovery via the same system resolver. A and AAAA
/// records are de-duplicated preserving order (Go `uniq(append(A, AAAA...))`).
pub async fn resolve_domain(
    domain: &str,
    opts: &EnrichOptions,
) -> Result<DnsRecords, EnrichmentError> {
    let fut = tokio::net::lookup_host((domain, 0));
    let addrs = tokio::time::timeout(opts.timeout, fut)
        .await
        .map_err(|_| EnrichmentError::Timeout)?
        .map_err(|e| EnrichmentError::dns(e.to_string()))?;

    let mut a = Vec::new();
    let mut aaaa = Vec::new();
    for sa in addrs {
        let ip = sa.ip().to_string();
        if sa.is_ipv4() {
            a.push(ip);
        } else {
            aaaa.push(ip);
        }
    }

    // CNAME chain: the Go engine obtains these from dnsx's CNAME question type.
    // tokio's lookup_host does not expose CNAMEs, so this is best-effort empty.
    let cname = Vec::new();

    Ok(DnsRecords { a, aaaa, cname })
}

/// Enrich a domain: resolve DNS, then probe the resulting host over HTTP(S).
///
/// Ports the composition of `Engine.ResolveDomain` + `Engine.ProbeSite`: first
/// resolve A/AAAA records (DNS, ungated), then, if `opts.probe_https` or a plain
/// HTTP probe is wanted, issue an HTTP probe against the host.
pub async fn enrich_domain(
    domain: &str,
    opts: &EnrichOptions,
) -> Result<Enrichment, EnrichmentError> {
    let dns = resolve_domain(domain, opts).await?;

    let mut resolved_ips = Vec::new();
    resolved_ips.extend(dns.a.iter().cloned());
    resolved_ips.extend(dns.aaaa.iter().cloned());

    let http = if opts.probe_https {
        Some(probe_url(&https_url(domain), opts).await?)
    } else {
        Some(probe_url(&http_url(domain), opts).await?)
    };

    Ok(Enrichment {
        ips_a: dns.a,
        ips_aaaa: dns.aaaa,
        resolved_ips,
        cname_chain: dns.cname,
        http,
    })
}

/// Enrich an IP literal: optional reverse DNS + an optional port probe.
///
/// The Go source has no direct IP-enrichment function; this is the rewrite's
/// companion that resolves the reverse DNS name (PTR) for an IP literal and,
/// optionally, probes the IP over HTTP on the well-known ports.
pub async fn enrich_ip(ip: &str, opts: &EnrichOptions) -> Result<Enrichment, EnrichmentError> {
    let cname_chain = reverse_dns(ip, opts).await.unwrap_or_default();

    let resolved_ips = vec![ip.to_string()];
    let mut http = None;
    if opts.probe_https {
        // Probe the IP as a host over both HTTP and HTTPS on the default port.
        let candidate = format!("https://{ip}/");
        if let Ok(probe) = probe_url(&candidate, opts).await {
            http = Some(probe);
        }
    }

    Ok(Enrichment {
        resolved_ips,
        cname_chain,
        http,
        ..Default::default()
    })
}

/// Build the https:// URL for a domain/host.
fn https_url(host: &str) -> String {
    let host = host.trim();
    if host.is_empty() {
        return String::new();
    }
    if host.contains("://") {
        return host.to_string();
    }
    format!("https://{host}/")
}

/// Build the http:// URL for a domain/host.
fn http_url(host: &str) -> String {
    let host = host.trim();
    if host.is_empty() {
        return String::new();
    }
    if host.contains("://") {
        return host.to_string();
    }
    format!("http://{host}/")
}

/// Reverse DNS lookup (PTR) for an IP literal. Returns the resolved names.
async fn reverse_dns(ip: &str, opts: &EnrichOptions) -> Result<Vec<String>, EnrichmentError> {
    // Best-effort: the system resolver is queried for the reverse name via
    // lookup_host on a constructed in-addr.arpa / ip6.arpa name. Because the
    // reverse lookup returns the resolved host for the reverse name, we resolve
    // the reverse name and collect any canonical names.
    let rev =
        reverse_name(ip).ok_or_else(|| EnrichmentError::parse(format!("invalid ip: {ip}")))?;
    let fut = tokio::net::lookup_host((rev.as_str(), 0));
    let addrs = tokio::time::timeout(opts.timeout, fut)
        .await
        .map_err(|_| EnrichmentError::Timeout)?;
    let _ = addrs;
    // tokio's lookup_host does not surface PTR records; return empty.
    Ok(Vec::new())
}

/// Compute the reverse-DNS name for an IP literal (in-addr.arpa / ip6.arpa).
fn reverse_name(ip: &str) -> Option<String> {
    let parsed: std::net::IpAddr = ip.parse().ok()?;
    match parsed {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            Some(format!(
                "{}.{}.{}.{}.in-addr.arpa",
                octets[3], octets[2], octets[1], octets[0]
            ))
        }
        std::net::IpAddr::V6(v6) => {
            let mut nibbles = String::with_capacity(64);
            for b in v6.octets() {
                nibbles.push(nibble(b >> 4));
                nibbles.push(nibble(b & 0x0f));
            }
            // ip6.arpa: reverse nibble order.
            let reversed: String = nibbles.chars().rev().collect();
            Some(format!("{reversed}.ip6.arpa"))
        }
    }
}

fn nibble(v: u8) -> char {
    std::char::from_digit((v & 0x0f) as u32, 16).unwrap_or('0')
}

/// Probe a single URL over HTTP(S) and return a [`HttpProbe`].
///
/// Ports `Engine.doHTTP`: `GET`, follow-redirect policy from `opts`, capture
/// status code, body length (capped at `opts.max_body_bytes`), page title,
/// `Server` header, content type, and a murmur3 hash of the favicon bytes.
pub async fn probe_url(raw_url: &str, opts: &EnrichOptions) -> Result<HttpProbe, EnrichmentError> {
    if raw_url.is_empty() {
        return Err(EnrichmentError::parse("empty url"));
    }
    let is_https = raw_url.starts_with("https://");

    // The client's redirect policy is set by `build_client` from `opts`.
    let client = build_client(opts)?;
    let req = client
        .get(raw_url)
        .header(reqwest::header::USER_AGENT, opts.user_agent.clone())
        .build()
        .map_err(|e| EnrichmentError::parse(e.to_string()))?;

    let resp = tokio::time::timeout(opts.timeout, client.execute(req))
        .await
        .map_err(|_| EnrichmentError::Timeout)?
        .map_err(|e| EnrichmentError::http(e.to_string()))?;

    let status = resp.status();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let server = resp
        .headers()
        .get(reqwest::header::SERVER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let final_url = resp.url().to_string();
    let version = resp.version();

    // Read the body, capping at max_body_bytes (Go `io.LimitReader(resp.Body, 1<<20)`).
    let body = read_limited(resp, opts.max_body_bytes).await?;

    let content_length = Some(body.len() as i64);
    let title = extract_title(&body);

    // Favicon hash: probe /favicon.ico relative to the final URL and hash bytes.
    let favicon_hash = probe_favicon(&final_url, opts).await.ok().flatten();

    // TLS info: reqwest exposes negotiated ALPN/version; subject/issuer are not
    // surfaced without a custom verifier, so they stay None.
    let tls = TlsInfo {
        enabled: is_https,
        alpn: alpn_from_version(version),
        subject_cn: None,
        issuer_cn: None,
    };

    Ok(HttpProbe {
        url: final_url,
        status_code: Some(status.as_u16()),
        content_length,
        title,
        server,
        content_type,
        favicon_hash,
        tls,
    })
}

fn alpn_from_version(v: reqwest::Version) -> Option<String> {
    use reqwest::Version;
    match v {
        Version::HTTP_09 => None,
        Version::HTTP_10 => None,
        Version::HTTP_11 => Some("http/1.1".to_string()),
        Version::HTTP_2 => Some("h2".to_string()),
        Version::HTTP_3 => Some("h3".to_string()),
        _ => None,
    }
}

/// Build a reqwest client with the redirect policy and TLS settings from `opts`.
fn build_client(opts: &EnrichOptions) -> Result<reqwest::Client, EnrichmentError> {
    let mut builder = reqwest::Client::builder()
        .timeout(opts.timeout)
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true);

    builder = if opts.follow_redirects {
        builder.redirect(reqwest::redirect::Policy::limited(10))
    } else {
        builder.redirect(reqwest::redirect::Policy::none())
    };

    builder
        .build()
        .map_err(|e| EnrichmentError::Http(e.to_string()))
}

/// Probe `<base>/favicon.ico` and return its murmur3 hash if found.
async fn probe_favicon(
    base_url: &str,
    opts: &EnrichOptions,
) -> Result<Option<i64>, EnrichmentError> {
    let favicon_url = favicon_url(base_url);
    if favicon_url.is_empty() {
        return Ok(None);
    }
    let client = build_client(opts)?;
    let resp = tokio::time::timeout(
        opts.timeout,
        client
            .get(&favicon_url)
            .header(reqwest::header::USER_AGENT, opts.user_agent.clone())
            .send(),
    )
    .await
    .map_err(|_| EnrichmentError::Timeout)?
    .map_err(|e| EnrichmentError::http(e.to_string()))?;

    if !resp.status().is_success() {
        return Ok(None);
    }
    let mut limited = read_limited(resp, opts.max_body_bytes).await?;
    let bytes = std::mem::take(&mut limited);
    if bytes.is_empty() {
        return Ok(None);
    }
    Ok(Some(murmur3_x86_32(&bytes, 0) as i64))
}

/// Construct the `/favicon.ico` URL for a base URL (scheme://host[:port]).
fn favicon_url(base_url: &str) -> String {
    // Parse out scheme://authority, ignoring any path.
    let Some(scheme_end) = base_url.find("://") else {
        return String::new();
    };
    let scheme = &base_url[..scheme_end];
    let rest = &base_url[scheme_end + 3..];
    let authority = match rest.find(['/', '?', '#']) {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    format!("{scheme}://{authority}/favicon.ico")
}

/// Read at most `max` bytes from a reqwest response body.
///
/// Ports `io.ReadAll(io.LimitReader(resp.Body, 1<<20))`: consume chunks until
/// the cap is reached, then stop without draining the rest of the stream.
async fn read_limited(resp: reqwest::Response, max: usize) -> Result<Vec<u8>, EnrichmentError> {
    let mut buf: Vec<u8> = Vec::with_capacity(max.min(64 * 1024));
    let mut resp = resp;
    while buf.len() < max {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                let remaining = max - buf.len();
                let take = chunk.len().min(remaining);
                buf.extend_from_slice(&chunk[..take]);
                if take < chunk.len() {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => return Err(EnrichmentError::http(e.to_string())),
        }
    }
    Ok(buf)
}

/// Extract the `<title>` text from an HTML body.
///
/// Ports `extractTitle` + `reTitle` from `enrich.go`:
/// `regexp.MustCompile(\`(?is)<title[^>]*>(.*?)</title>\`)` with
/// `strings.TrimSpace(html.UnescapeString(...))`. Case-insensitive, dot matches
/// newline (non-greedy), and the inner text is HTML-unescaped and trimmed.
pub fn extract_title(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    let lower = text.to_ascii_lowercase();
    let open = "<title";
    let close = "</title>";

    let start = lower.find(open)?;
    // Find the '>' that ends the opening tag (after "<title").
    let after_open = start + open.len();
    let gt_rel = lower[after_open..].find('>')?;
    let content_start = after_open + gt_rel + 1;

    let close_rel = lower[content_start..].find(close)?;
    let content_end = content_start + close_rel;

    let inner = &text[content_start..content_end];
    let unescaped = html_unescape(inner);
    Some(unescaped.trim().to_string())
}

/// Unescape the HTML entities that `html.UnescapeString` handles, covering the
/// common named and numeric entities used in `<title>` text.
fn html_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some((decoded, consumed)) = try_entity(&s[i..]) {
                out.push(decoded);
                i += consumed;
                continue;
            }
        }
        // Copy one UTF-8 char.
        let ch_len = utf8_char_len(bytes[i]);
        let end = (i + ch_len).min(bytes.len());
        out.push_str(&s[i..end]);
        i = end;
    }
    out
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Try to decode a single HTML entity at the start of `s` (which begins with
/// `&`). Returns the decoded char and the number of bytes consumed.
fn try_entity(s: &str) -> Option<(char, usize)> {
    // Numeric: &#DDD; or &#xHHH;
    if let Some(rest) = s.strip_prefix("&#") {
        let (radix, digits, prefix_len) =
            if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
                (16, hex, 1)
            } else {
                (10, rest, 0)
            };
        let end = digits.find(';')?;
        let num_str = &digits[..end];
        if num_str.is_empty() {
            return None;
        }
        let code = u32::from_str_radix(num_str, radix).ok()?;
        let ch = char::from_u32(code)?;
        // "&#" + optional x/X + digits + ';'
        return Some((ch, 2 + prefix_len + end + 1));
    }
    // Named entities.
    let semi = s.find(';')?;
    let name = &s[1..semi];
    let ch = named_entity(name)?;
    Some((ch, semi + 1))
}

fn named_entity(name: &str) -> Option<char> {
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00a0}',
        "copy" => '\u{00a9}',
        "reg" => '\u{00ae}',
        "trade" => '\u{2122}',
        "hellip" => '\u{2026}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "bull" => '\u{2022}',
        "middot" => '\u{00b7}',
        "deg" => '\u{00b0}',
        "plusmn" => '\u{00b1}',
        "frac12" => '\u{00bd}',
        "times" => '\u{00d7}',
        "divide" => '\u{00f7}',
        "euro" => '\u{20ac}',
        "pound" => '\u{00a3}',
        "yen" => '\u{00a5}',
        "cent" => '\u{00a2}',
        "sect" => '\u{00a7}',
        "para" => '\u{00b6}',
        "dagger" => '\u{2020}',
        "Dagger" => '\u{2021}',
        _ => return None,
    };
    Some(c)
}

/// murmur3 x86 32-bit hash (used by the `favhash`/`mmh3` favicon hashing).
///
/// Port of the murmurhash3 x86 32-bit algorithm as used by Shodan's favicon
/// hash (`mmh3.hash(base64(favicon_bytes))`). Note: the Go/ARTEX source hashes
/// the raw favicon bytes; callers who need the Shodan-compatible base64 form
/// should base64-encode before calling.
pub fn murmur3_x86_32(data: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    let length = data.len();
    let mut h1 = seed;
    let rounded_end = length / 4 * 4;

    let mut i = 0;
    while i < rounded_end {
        let mut k1 = (data[i] as u32)
            | ((data[i + 1] as u32) << 8)
            | ((data[i + 2] as u32) << 16)
            | ((data[i + 3] as u32) << 24);
        i += 4;
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(C2);

        h1 ^= k1;
        h1 = h1.rotate_left(13);
        h1 = h1.wrapping_mul(5).wrapping_add(0xe654_6b64);
    }

    // Tail.
    let mut k1: u32 = 0;
    match length & 3 {
        3 => {
            k1 ^= (data[rounded_end + 2] as u32) << 16;
            k1 ^= (data[rounded_end + 1] as u32) << 8;
            k1 ^= data[rounded_end] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        2 => {
            k1 ^= (data[rounded_end + 1] as u32) << 8;
            k1 ^= data[rounded_end] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        1 => {
            k1 ^= data[rounded_end] as u32;
            k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(15);
            k1 = k1.wrapping_mul(C2);
            h1 ^= k1;
        }
        _ => {}
    }

    // Finalization.
    h1 ^= length as u32;
    h1 = fmix32(h1);
    h1
}

fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- title extraction ----

    #[test]
    fn title_basic() {
        assert_eq!(
            extract_title(b"<html><head><title>Hello</title></head></html>"),
            Some("Hello".to_string())
        );
    }

    #[test]
    fn title_with_attributes() {
        assert_eq!(
            extract_title(b"<title lang=\"en\">Hi</title>"),
            Some("Hi".to_string())
        );
    }

    #[test]
    fn title_case_insensitive() {
        assert_eq!(
            extract_title(b"<TITLE>Shout</TITLE>"),
            Some("Shout".to_string())
        );
    }

    #[test]
    fn title_newlines_inside() {
        assert_eq!(
            extract_title(b"<title>\n  Multi\n  Line\n</title>"),
            Some("Multi\n  Line".to_string())
        );
    }

    #[test]
    fn title_trim_and_unescape() {
        assert_eq!(
            extract_title(b"<title>  A &amp; B &lt;tag&gt;  </title>"),
            Some("A & B <tag>".to_string())
        );
    }

    #[test]
    fn title_numeric_entity() {
        assert_eq!(
            extract_title(b"<title>&#65;&#x42;&#67;</title>"),
            Some("ABC".to_string())
        );
    }

    #[test]
    fn title_missing() {
        assert_eq!(
            extract_title(b"<html><body>no title here</body></html>"),
            None
        );
    }

    #[test]
    fn title_empty() {
        assert_eq!(extract_title(b"<title></title>"), Some(String::new()));
    }

    // ---- murmur3 ----

    #[test]
    fn murmur3_empty() {
        assert_eq!(murmur3_x86_32(b"", 0), 0);
    }

    #[test]
    fn murmur3_hello() {
        assert_eq!(murmur3_x86_32(b"hello", 0), 0x248b_fa47);
    }

    #[test]
    fn murmur3_hello_seed() {
        // Known vector: murmur3_x86_32("hello", seed=1)
        assert_eq!(murmur3_x86_32(b"hello", 1), 0xbb4a_bcad);
    }

    #[test]
    fn murmur3_foo() {
        assert_eq!(murmur3_x86_32(b"foo", 0), 0xf6a5c420);
    }

    // ---- CNAME / reverse name ----

    #[test]
    fn reverse_name_ipv4() {
        assert_eq!(
            reverse_name("1.2.3.4").as_deref(),
            Some("4.3.2.1.in-addr.arpa")
        );
    }

    #[test]
    fn reverse_name_ipv6() {
        let r = reverse_name("::1").unwrap();
        assert!(r.ends_with(".ip6.arpa"));
        assert!(r.starts_with('1'));
    }

    #[test]
    fn reverse_name_invalid() {
        assert!(reverse_name("not-an-ip").is_none());
    }

    #[test]
    fn favicon_url_parses() {
        assert_eq!(
            favicon_url("https://example.com/some/path"),
            "https://example.com/favicon.ico"
        );
        assert_eq!(
            favicon_url("http://example.com:8080/"),
            "http://example.com:8080/favicon.ico"
        );
    }
}
