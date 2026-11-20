//! JWT auth, password hashing and login rate limiting — port of ARTEX
//! `server/auth.go` + `server/login_limit.go`.
//!
//! Framework-agnostic: token sign/verify, password validation, the per-IP login
//! limiter and client-IP resolution. The axum wiring (request extraction, JSON
//! responses) lives in the `api` crate.

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const JWT_KEY_FILENAME: &str = "jwt.key";
pub const AUTH_PASS_KEY: &str = "auth.password_hash";
pub const JWT_TTL_DAYS: u64 = 7;
pub const MIN_PASSWORD_RUNES: usize = 12;
pub const MAX_PASSWORD_BYTES: usize = 72;
pub const USERNAME: &str = "ARTEX";

const KEY_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("jwt key {path} is shorter than 32 bytes; fix or remove it (it is never overwritten)")]
    ShortKey { path: String },
    #[error("jwt: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("bcrypt: {0}")]
    Bcrypt(#[from] bcrypt::BcryptError),
}

/// Claims for the ARTEX session token (HS256, subject "ARTEX").
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
    iat: usize,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load or create the 32-byte signing key at `key_dir/jwt.key`.
///
/// Migrates a legacy key from `data_dir/jwt.key` then `base_dir/jwt.key`
/// (preserving it so sessions stay valid), otherwise generates a random one.
/// An existing key file is never overwritten.
pub fn load_or_create_jwt_key(
    key_dir: &Path,
    data_dir: &Path,
    base_dir: &Path,
) -> Result<Vec<u8>, AuthError> {
    std::fs::create_dir_all(key_dir)?;
    let path = key_dir.join(JWT_KEY_FILENAME);
    if !path.exists() {
        migrate_jwt_key(&path, data_dir, base_dir)?;
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let key = s.trim().to_string();
            if key.len() < 32 {
                return Err(AuthError::ShortKey {
                    path: path.display().to_string(),
                });
            }
            Ok(key.into_bytes())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = generate_key();
            write_new_jwt_key(&path, &key)?;
            tracing::info!(path = %path.display(), "new JWT key written");
            Ok(key)
        }
        Err(e) => Err(AuthError::Io(e)),
    }
}

fn generate_key() -> Vec<u8> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| KEY_CHARS[rng.gen_range(0..KEY_CHARS.len())])
        .collect()
}

fn write_new_jwt_key(path: &Path, data: &[u8]) -> Result<(), AuthError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    if let Err(e) = f.write_all(data) {
        let _ = std::fs::remove_file(path);
        return Err(AuthError::Io(e));
    }
    Ok(())
}

fn migrate_jwt_key(dst: &Path, data_dir: &Path, base_dir: &Path) -> Result<(), AuthError> {
    for src in [
        data_dir.join(JWT_KEY_FILENAME),
        base_dir.join(JWT_KEY_FILENAME),
    ] {
        if same_path(&src, dst) {
            continue;
        }
        match std::fs::read(&src) {
            Ok(data) => {
                write_new_jwt_key(dst, &data)?;
                if let Err(e) = std::fs::remove_file(&src) {
                    tracing::warn!(src = %src.display(), err = %e, "JWT key copied but old file could not be removed");
                }
                tracing::info!(src = %src.display(), dst = %dst.display(), "JWT key migrated");
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::warn!(src = %src.display(), err = %e, "cannot read legacy JWT key");
                continue;
            }
        }
    }
    Ok(())
}

fn same_path(a: &Path, b: &Path) -> bool {
    let abs = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    abs(a) == abs(b)
}

/// Issue a 7-day HS256 token for user ARTEX.
pub fn sign_jwt(key: &[u8]) -> Result<String, AuthError> {
    let now = now_unix() as usize;
    let claims = Claims {
        sub: USERNAME.to_string(),
        exp: now + (JWT_TTL_DAYS as usize * 24 * 3600),
        iat: now,
    };
    Ok(encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(key),
    )?)
}

/// Validate an HS256 token; returns the subject when valid and non-expired.
pub fn verify_jwt(token: &str, key: &[u8]) -> Option<String> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    match decode::<Claims>(token, &DecodingKey::from_secret(key), &validation) {
        Ok(data) if data.claims.sub == USERNAME => Some(data.claims.sub),
        _ => None,
    }
}

/// Returns true when `token` is a valid, non-expired HS256 token.
pub fn is_valid_jwt(token: &str, key: &[u8]) -> bool {
    verify_jwt(token, key).is_some()
}

/// Validate a new password; returns an error message when invalid, else None.
pub fn validate_password(pw: &str) -> Option<String> {
    if pw.chars().count() < MIN_PASSWORD_RUNES {
        return Some(format!(
            "Password must be at least {} characters",
            MIN_PASSWORD_RUNES
        ));
    }
    if pw.len() > MAX_PASSWORD_BYTES {
        return Some(format!(
            "Password must not exceed {} bytes",
            MAX_PASSWORD_BYTES
        ));
    }
    None
}

/// Hash a password with bcrypt default cost.
pub fn hash_password(pw: &str) -> Result<String, AuthError> {
    Ok(bcrypt::hash(pw, bcrypt::DEFAULT_COST)?)
}

/// Verify a password against a bcrypt hash.
pub fn verify_password(pw: &str, hash: &str) -> bool {
    bcrypt::verify(pw, hash).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Login rate limiter — port of server/login_limit.go
// ---------------------------------------------------------------------------

const LOGIN_MAX_ATTEMPTS: u32 = 10;
const LOGIN_WINDOW: Duration = Duration::from_secs(60);
const LOGIN_BASE_BLOCK: Duration = Duration::from_secs(60);
const LOGIN_MAX_BLOCK: Duration = Duration::from_secs(15 * 60);
const LOGIN_SWEEP_EVERY: Duration = Duration::from_secs(60);
const LOGIN_MAX_ENTRIES: usize = 10_000;

#[derive(Default)]
struct LoginEntry {
    attempts: u32,
    window_start: Option<SystemTime>,
    blocked_until: Option<SystemTime>,
    block: Duration,
    last_seen: Option<SystemTime>,
}

/// Per-IP login attempt limiter (in-memory). State is cleared on restart.
#[derive(Default)]
pub struct LoginLimiter {
    entries: HashMap<String, LoginEntry>,
    last_sweep: Option<SystemTime>,
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one attempt from `ip`. Returns `Some(wait)` when blocked, else None.
    pub fn allow(&mut self, ip: &str) -> Option<Duration> {
        let now = SystemTime::now();
        self.sweep(now);
        let e = self
            .entries
            .entry(ip.to_string())
            .or_insert_with(|| LoginEntry {
                window_start: Some(now),
                ..Default::default()
            });
        e.last_seen = Some(now);

        if let Some(until) = e.blocked_until {
            if now < until {
                e.block = std::cmp::min(e.block * 2, LOGIN_MAX_BLOCK);
                e.blocked_until = Some(now + e.block);
                return Some(e.block);
            }
        }

        let window_start = e.window_start.unwrap_or(now);
        if now.duration_since(window_start).unwrap_or_default() >= LOGIN_WINDOW {
            e.attempts = 0;
            e.window_start = Some(now);
        }

        if e.attempts >= LOGIN_MAX_ATTEMPTS {
            e.block = if e.block.is_zero() {
                LOGIN_BASE_BLOCK
            } else {
                std::cmp::min(e.block * 2, LOGIN_MAX_BLOCK)
            };
            e.blocked_until = Some(now + e.block);
            e.attempts = 0;
            e.window_start = Some(now);
            return Some(e.block);
        }

        e.attempts += 1;
        None
    }

    /// Forget an IP after a successful login.
    pub fn reset(&mut self, ip: &str) {
        self.entries.remove(ip);
    }

    fn sweep(&mut self, now: SystemTime) {
        if let Some(last) = self.last_sweep {
            if now.duration_since(last).unwrap_or_default() < LOGIN_SWEEP_EVERY {
                return;
            }
        }
        self.last_sweep = Some(now);
        self.entries.retain(|_, e| {
            e.last_seen
                .map(|ls| now.duration_since(ls).unwrap_or_default() < LOGIN_MAX_BLOCK)
                .unwrap_or(false)
        });
        // Hard cap on tracked IPs (matches Go's evictOldest when over the cap).
        while self.entries.len() > LOGIN_MAX_ENTRIES {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_seen)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

/// Resolve the client IP for login counting. When the peer is a trusted proxy,
/// X-Forwarded-For is read from the right past trusted hops; the first untrusted
/// hop is the client. With no trusted proxies, the peer is used directly.
pub fn client_ip(peer: &SocketAddr, xff_values: &[String], trusted: &[TrustedProxy]) -> String {
    let mut addr = peer.ip();
    if !is_trusted(addr, trusted) {
        return addr.to_string();
    }
    let joined = xff_values.join(",");
    let hops: Vec<&str> = joined.split(',').collect();
    for i in (0..hops.len()).rev() {
        match hops[i].trim().parse::<IpAddr>() {
            Ok(hop) => {
                addr = hop;
                if !is_trusted(addr, trusted) {
                    break;
                }
            }
            Err(_) => break, // keep the last address a trusted proxy reported
        }
    }
    addr.to_string()
}

/// A trusted proxy network (CIDR or single host).
#[derive(Debug, Clone, PartialEq)]
pub enum TrustedProxy {
    Single(IpAddr),
    Network { addr: IpAddr, prefix: u8 },
}

impl TrustedProxy {
    fn contains(&self, ip: IpAddr) -> bool {
        match self {
            TrustedProxy::Single(a) => *a == ip,
            TrustedProxy::Network { addr, prefix } => match (addr, ip) {
                (IpAddr::V4(net), IpAddr::V4(ip)) => {
                    let net = u32::from(*net);
                    let ip = u32::from(ip);
                    let mask = if *prefix == 0 {
                        0
                    } else {
                        u32::MAX << (32 - *prefix)
                    };
                    (net & mask) == (ip & mask)
                }
                (IpAddr::V6(net), IpAddr::V6(ip)) => {
                    let net = u128::from(*net);
                    let ip = u128::from(ip);
                    let mask = if *prefix == 0 {
                        0
                    } else {
                        u128::MAX << (128 - *prefix)
                    };
                    (net & mask) == (ip & mask)
                }
                _ => false,
            },
        }
    }
}

fn is_trusted(ip: IpAddr, trusted: &[TrustedProxy]) -> bool {
    trusted.iter().any(|t| t.contains(ip))
}

/// Parse `ARTEX_TRUSTED_PROXIES`: comma-separated CIDRs (a bare address means
/// that one host). Empty means no trusted proxies.
pub fn parse_trusted_proxies(s: &str) -> Result<Vec<TrustedProxy>, String> {
    let mut out = Vec::new();
    for f in s.split(',') {
        let f = f.trim();
        if f.is_empty() {
            continue;
        }
        if let Some((addr_str, prefix_str)) = f.split_once('/') {
            let addr: IpAddr = addr_str
                .parse()
                .map_err(|_| format!("{:?} is not a CIDR or an IP address", f))?;
            let prefix: u8 = prefix_str
                .parse()
                .map_err(|_| format!("{:?} is not a CIDR or an IP address", f))?;
            let max = if addr.is_ipv4() { 32 } else { 128 };
            if prefix > max {
                return Err(format!("{:?} is not a CIDR or an IP address", f));
            }
            out.push(TrustedProxy::Network { addr, prefix });
        } else {
            let addr: IpAddr = f
                .parse()
                .map_err(|_| format!("{:?} is not a CIDR or an IP address", f))?;
            out.push(TrustedProxy::Single(addr));
        }
    }
    Ok(out)
}

/// Constant-time comparison of the setup token.
pub fn check_setup_token(got: &str, expected: &str) -> bool {
    use subtle::ConstantTimeEq;
    if expected.is_empty() {
        return false;
    }
    bool::from(got.as_bytes().ct_eq(expected.as_bytes()))
}

/// Generate a 16-byte hex setup token.
pub fn generate_setup_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_validation() {
        assert!(validate_password("short").is_some());
        assert!(validate_password("this is long enough").is_none());
        // 72-byte limit.
        let too_long = "a".repeat(73);
        assert!(validate_password(&too_long).is_some());
    }

    #[test]
    fn jwt_roundtrip() {
        let key = b"0123456789012345678901234567890123456789";
        let tok = sign_jwt(key).unwrap();
        assert_eq!(verify_jwt(&tok, key).as_deref(), Some("ARTEX"));
        assert!(!is_valid_jwt(&tok, b"a-different-key"));
    }

    #[test]
    fn bcrypt_roundtrip() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn login_limiter_blocks_after_max() {
        let mut l = LoginLimiter::new();
        for _ in 0..LOGIN_MAX_ATTEMPTS {
            assert!(l.allow("1.2.3.4").is_none());
        }
        // Next attempt is blocked.
        assert!(l.allow("1.2.3.4").is_some());
    }

    #[test]
    fn login_limiter_reset_clears() {
        let mut l = LoginLimiter::new();
        for _ in 0..LOGIN_MAX_ATTEMPTS {
            l.allow("1.2.3.4");
        }
        l.reset("1.2.3.4");
        assert!(l.allow("1.2.3.4").is_none());
    }

    #[test]
    fn parse_proxies() {
        let t = parse_trusted_proxies("127.0.0.1/32, 10.0.0.0/8, 192.168.1.1").unwrap();
        assert_eq!(t.len(), 3);
        assert!(parse_trusted_proxies("not-an-ip").is_err());
    }

    #[test]
    fn client_ip_trusted() {
        let peer: SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let trusted = parse_trusted_proxies("127.0.0.1/32").unwrap();
        // Peer is trusted; XFF says the real client is 8.8.8.8.
        let xff = vec!["8.8.8.8".to_string()];
        assert_eq!(client_ip(&peer, &xff, &trusted), "8.8.8.8");
        // Untrusted peer: XFF ignored.
        let peer2: SocketAddr = "9.9.9.9:5555".parse().unwrap();
        assert_eq!(client_ip(&peer2, &xff, &trusted), "9.9.9.9");
    }

    #[test]
    fn setup_token_compare() {
        assert!(check_setup_token("abc", "abc"));
        assert!(!check_setup_token("abc", "abd"));
        assert!(!check_setup_token("abc", ""));
    }
}
