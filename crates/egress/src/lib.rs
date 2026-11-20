//! HTTP noise governor — behaviour-preserving port of ARTEX `egress/egress.go`.
//!
//! The governor paces platform-owned HTTP requests to a target (WebFetch, the
//! `http` custom tools, the enrich probe). A [`Profile`] (from
//! `ARTEX_NOISE_PROFILE`) sets every lever: a per-host token bucket, a per-send
//! jitter, a per-host and a global in-flight concurrency cap, a backoff after a
//! 429/503, and, for the GET requests of one task, coalescing of identical
//! in-flight requests plus a short response cache.
//!
//! The zero profile (`off`) is a strict passthrough: [`Governor::acquire`]
//! returns a no-op [`Permit`] and [`Governor::do_get`] runs the request
//! directly, so requests and timing are unchanged.
//!
//! There is no HTTP client in this crate. A future `reqwest`-based transport
//! calls [`Governor::acquire`] (or [`Governor::acquire_url`]) before a send and
//! holds the returned [`Permit`] until the response body is closed, and calls
//! [`Governor::do_get`] for the GET path that coalesces and caches.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use url::Url;

/// Caps the jitter of one send (Go `MaxJitter`).
pub const MAX_JITTER: Duration = Duration::from_millis(250);

/// Errors returned by the governor.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EgressError {
    /// A semaphore was closed; the governor is shutting down.
    #[error("egress: semaphore closed")]
    Unavailable,
    /// A request-level failure reported by the caller's closure.
    #[error("egress: {0}")]
    Request(String),
    /// The leader's response was not shareable (for example too large).
    ///
    /// A caller that waited on a flight failed with this error runs its own
    /// request, uncached (Go `errNotShared`).
    #[error("egress: response not shared")]
    NotShared,
    /// The leader was cancelled; waiters run their own request (Go
    /// `context.Canceled` / `context.DeadlineExceeded`).
    #[error("egress: request cancelled")]
    Cancelled,
}

impl EgressError {
    /// A request-level error.
    pub fn request(message: impl Into<String>) -> Self {
        Self::Request(message.into())
    }

    /// Whether only the leader may see this error: a waiter re-runs its own
    /// request instead of sharing the failure.
    pub fn is_leader_only(&self) -> bool {
        matches!(self, Self::NotShared | Self::Cancelled)
    }
}

/// One noise level. The zero profile (`off`) is a passthrough.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// Profile name (`off`, `quiet`, `balanced`, `aggressive`).
    pub name: String,
    /// Token refill per host, requests per second; `0` = off.
    pub rps: f64,
    /// Bucket size per host.
    pub burst: u32,
    /// Requests in flight per host.
    pub max_inflight_per_host: u32,
    /// Requests in flight across all hosts.
    pub max_inflight_global: u32,
    /// Each send waits a uniform `[0, min(Jitter/RPS, MAX_JITTER)]`.
    pub jitter: f64,
    /// Per-task cache of identical GETs; zero = coalescing only.
    pub cache_ttl: Duration,
    /// Retries after a 429/503 (bodiless GET/HEAD/OPTIONS only).
    pub max_retries: u32,
    /// Backoff without `Retry-After`, doubled per retry.
    pub base_delay: Duration,
    /// Cap of one backoff; a longer `Retry-After` is not retried.
    pub max_delay: Duration,
    /// `ARTEX_TARGET_USER_AGENT`, set on requests that have none; empty =
    /// unchanged.
    pub user_agent: String,
}

impl Default for Profile {
    fn default() -> Self {
        Self::off()
    }
}

impl Profile {
    /// The passthrough profile: no rate, jitter or concurrency limit.
    pub fn off() -> Self {
        Self {
            name: "off".to_string(),
            rps: 0.0,
            burst: 0,
            max_inflight_per_host: 0,
            max_inflight_global: 0,
            jitter: 0.0,
            cache_ttl: Duration::ZERO,
            max_retries: 0,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            user_agent: String::new(),
        }
    }

    /// The `quiet` profile: 2 rps, burst 4, one in flight per host.
    pub fn quiet() -> Self {
        Self {
            name: "quiet".to_string(),
            rps: 2.0,
            burst: 4,
            max_inflight_per_host: 1,
            max_inflight_global: 8,
            jitter: 0.5,
            cache_ttl: Duration::from_secs(60),
            max_retries: 3,
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(60),
            user_agent: String::new(),
        }
    }

    /// The `balanced` profile: 5 rps, burst 10, two in flight per host.
    pub fn balanced() -> Self {
        Self {
            name: "balanced".to_string(),
            rps: 5.0,
            burst: 10,
            max_inflight_per_host: 2,
            max_inflight_global: 16,
            jitter: 0.2,
            cache_ttl: Duration::from_secs(30),
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            user_agent: String::new(),
        }
    }

    /// The `aggressive` profile: 20 rps, burst 40, four in flight per host.
    pub fn aggressive() -> Self {
        Self {
            name: "aggressive".to_string(),
            rps: 20.0,
            burst: 40,
            max_inflight_per_host: 4,
            max_inflight_global: 32,
            jitter: 0.1,
            cache_ttl: Duration::from_secs(10),
            max_retries: 2,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
            user_agent: String::new(),
        }
    }

    /// The profile a name selects; unset, `off` and unknown names are off
    /// (Go `ProfileFromEnv` lookup).
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "quiet" => Self::quiet(),
            "balanced" => Self::balanced(),
            "aggressive" => Self::aggressive(),
            _ => Self::off(),
        }
    }

    /// The profile `ARTEX_NOISE_PROFILE` names; unset, `off` or unknown is off.
    ///
    /// `ARTEX_TARGET_USER_AGENT` is trimmed and applied only when it has no
    /// control character.
    pub fn from_env() -> Self {
        let raw = std::env::var("ARTEX_NOISE_PROFILE").unwrap_or_default();
        let name = raw.trim().to_ascii_lowercase();
        let mut profile = Self::from_name(&name);
        if profile.is_off() && !name.is_empty() && name != "off" {
            eprintln!(
                "[egress] ARTEX_NOISE_PROFILE={name:?} is not off, quiet, balanced or aggressive; using off"
            );
        }
        if !profile.is_off() {
            let ua = std::env::var("ARTEX_TARGET_USER_AGENT").unwrap_or_default();
            let ua = ua.trim();
            if ua.chars().any(char::is_control) {
                eprintln!("[egress] ARTEX_TARGET_USER_AGENT contains a control character; ignored");
            } else {
                profile.user_agent = ua.to_string();
            }
        }
        profile
    }

    /// Whether this profile disables the governor (Go `Governor.off`).
    pub fn is_off(&self) -> bool {
        self.rps <= 0.0
    }
}

/// The outcome of a `Retry-After` / backoff computation (Go `Governor.backoff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryAfter {
    /// How long to wait before the next attempt.
    pub delay: Duration,
    /// Whether the request may be retried at all: `false` when `Retry-After`
    /// asks for more than the profile's `max_delay`.
    pub retry: bool,
}

/// The host key of a URL: the lowercased host, with the port when it is not
/// the default (Go `HostKey`).
pub fn host_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    match url.port() {
        None => host,
        Some(port)
            if (url.scheme().eq_ignore_ascii_case("http") && port == 80)
                || (url.scheme().eq_ignore_ascii_case("https") && port == 443) =>
        {
            host
        }
        Some(port) if host.contains(':') => format!("[{host}]:{port}"),
        Some(port) => format!("{host}:{port}"),
    }
}

/// The one governor of a process, built from the environment by
/// [`Governor::shared`] (Go `Shared`).
pub struct Governor {
    profile: Profile,
    global: Arc<Semaphore>,
    hosts: Mutex<HashMap<String, Arc<HostState>>>,
    flights: Mutex<HashMap<String, Arc<Entry>>>,
}

impl Governor {
    /// Builds a governor for `profile`.
    pub fn new(profile: Profile) -> Self {
        let global = Arc::new(Semaphore::new(
            usize::try_from(profile.max_inflight_global.max(1)).unwrap_or(1),
        ));
        Self {
            profile,
            global,
            hosts: Mutex::new(HashMap::new()),
            flights: Mutex::new(HashMap::new()),
        }
    }

    /// The process-wide governor, built from the environment on first use.
    pub fn shared() -> &'static Governor {
        static SHARED: OnceLock<Governor> = OnceLock::new();
        SHARED.get_or_init(|| Governor::new(Profile::from_env()))
    }

    /// The profile this governor enforces.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Whether the governor is off (a strict passthrough).
    pub fn is_off(&self) -> bool {
        self.profile.is_off()
    }

    /// The bound of one send's jitter: `min(Jitter/RPS, MAX_JITTER)`.
    fn jitter_bound(&self) -> Duration {
        if self.profile.rps <= 0.0 {
            return Duration::ZERO;
        }
        Duration::try_from_secs_f64(self.profile.jitter / self.profile.rps)
            .unwrap_or(Duration::MAX)
            .min(MAX_JITTER)
    }

    /// A uniform jitter in `[0, jitter_bound]`.
    fn jitter(&self) -> Duration {
        let nanos = self.jitter_bound().as_nanos();
        if nanos == 0 {
            return Duration::ZERO;
        }
        let bound = u64::try_from(nanos).unwrap_or(u64::MAX);
        Duration::from_nanos(next_rand() % (bound.saturating_add(1)))
    }

    /// The per-host state, created on first use.
    fn host(&self, key: &str) -> Arc<HostState> {
        let mut hosts = lock(&self.hosts);
        Arc::clone(hosts.entry(key.to_string()).or_insert_with(|| {
            Arc::new(HostState {
                slots: Arc::new(Semaphore::new(
                    usize::try_from(self.profile.max_inflight_per_host.max(1)).unwrap_or(1),
                )),
                inner: Mutex::new(HostInner {
                    tokens: self.profile.burst as f64,
                    last: Instant::now(),
                    not_before: Instant::now(),
                    sent: 0,
                    dupes: 0,
                    inflight: 0,
                    peak_inflight: 0,
                    peak_rpm: 0,
                }),
            })
        }))
    }

    /// Takes a token from `host`'s bucket and returns how long to wait for it
    /// (Go `Governor.reserve`).
    fn reserve(&self, host: &HostState) -> Duration {
        let mut inner = lock(&host.inner);
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(inner.last).as_secs_f64();
        inner.tokens =
            (self.profile.burst as f64).min(inner.tokens + elapsed * self.profile.rps) - 1.0;
        inner.last = now;
        let token_wait = (-inner.tokens / self.profile.rps).max(0.0);
        let wait = Duration::try_from_secs_f64(token_wait).unwrap_or(Duration::MAX);
        wait.max(inner.not_before.saturating_duration_since(now))
    }

    /// Waits for a host slot, a token of the host's bucket (or the end of its
    /// backoff) plus the jitter, then a global slot, taken last so a waiting
    /// host holds no global slot (Go `Governor.acquire`).
    ///
    /// The returned [`Permit`] frees both slots when dropped; hold it until the
    /// response body is closed.
    pub async fn acquire(&self, host: &str) -> Result<Permit, EgressError> {
        if self.is_off() {
            return Ok(Permit::passthrough());
        }
        let state = self.host(host);
        let host_permit = state
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| EgressError::Unavailable)?;
        let wait = self.reserve(&state).saturating_add(self.jitter());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        let global_permit = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| EgressError::Unavailable)?;
        {
            let mut inner = lock(&state.inner);
            inner.sent += 1;
            inner.inflight += 1;
            inner.peak_inflight = inner.peak_inflight.max(inner.inflight);
        }
        Ok(Permit {
            _global: Some(global_permit),
            _host: Some(host_permit),
            state: Some(state),
        })
    }

    /// [`Governor::acquire`] for a URL, keyed by [`host_key`].
    pub async fn acquire_url(&self, url: &Url) -> Result<Permit, EgressError> {
        self.acquire(&host_key(url)).await
    }

    /// Pauses `host` until `delay` from now, capped at the profile's
    /// `max_delay` (Go `send` after a 429/503).
    pub fn penalize(&self, host: &str, delay: Duration) {
        if self.is_off() {
            return;
        }
        let state = self.host(host);
        let delay = delay.min(self.profile.max_delay);
        let mut inner = lock(&state.inner);
        if let Some(until) = Instant::now().checked_add(delay) {
            if until > inner.not_before {
                inner.not_before = until;
            }
        }
    }

    /// The backoff after a 429/503: `Retry-After` (seconds or HTTP date), else
    /// `base_delay` doubled per retry, capped at `max_delay`.
    ///
    /// `retry` is `false` when `Retry-After` asks for more than `max_delay`.
    pub fn retry_after(&self, header: Option<&str>, attempt: u32) -> RetryAfter {
        if let Some(value) = header.map(str::trim).filter(|value| !value.is_empty()) {
            if let Ok(seconds) = value.parse::<i64>() {
                if seconds >= 0 {
                    let delay = Duration::from_secs(seconds as u64);
                    return RetryAfter {
                        delay,
                        retry: delay <= self.profile.max_delay,
                    };
                }
            }
            if let Some(when) = parse_http_date(value) {
                let delay = when
                    .duration_since(SystemTime::now())
                    .unwrap_or(Duration::ZERO);
                return RetryAfter {
                    delay,
                    retry: delay <= self.profile.max_delay,
                };
            }
        }
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        let delay = self
            .profile
            .base_delay
            .checked_mul(factor)
            .filter(|delay| !delay.is_zero() && *delay <= self.profile.max_delay)
            .unwrap_or(self.profile.max_delay);
        RetryAfter { delay, retry: true }
    }

    /// Whether a status is retried after a backoff (Go `send`).
    pub const fn is_retryable_status(status: u16) -> bool {
        matches!(status, 429 | 503)
    }

    /// Whether a method is bodiless/idempotent and may be retried (Go `send`).
    pub fn is_idempotent(method: &str) -> bool {
        matches!(method, "GET" | "HEAD" | "OPTIONS")
    }

    /// Whether a request may be retried: a 429/503, an idempotent method, and
    /// fewer than `max_retries` retries so far.
    pub fn should_retry(&self, method: &str, status: u16, attempt: u32) -> bool {
        Self::is_retryable_status(status)
            && Self::is_idempotent(method)
            && attempt < self.profile.max_retries
    }

    /// Runs `f` as one GET of one task, coalescing identical in-flight GETs and
    /// keeping the result for the TTL `f` returns (Go `Governor.share` through
    /// `transport.RoundTrip`).
    ///
    /// `namespace` names the client (a custom tool's key, `enrich`); it is part
    /// of the key, so clients with other proxies, gates or pins never share a
    /// response. `headers` are folded into the key in sorted order. With no
    /// `task`, or with the governor off, `f` just runs: nothing is coalesced or
    /// cached.
    ///
    /// `f` returns the value and how long it may be cached; a zero TTL
    /// coalesces but never caches (the 429/5xx case).
    pub async fn do_get<T, F, Fut>(
        &self,
        namespace: &str,
        task: &str,
        url: &Url,
        headers: &[(String, String)],
        f: F,
    ) -> Result<T, EgressError>
    where
        T: Clone + Send + Sync + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(T, Duration), EgressError>>,
    {
        if self.is_off() || task.is_empty() {
            return f().await.map(|(value, _)| value);
        }
        let key = share_key(namespace, task, url, headers);
        let (entry, leader) = {
            let mut flights = lock(&self.flights);
            let now = Instant::now();
            flights.retain(|_, entry| !entry.expired(now));
            if let Some(entry) = flights.get(&key) {
                (Arc::clone(entry), false)
            } else {
                let (tx, _) = watch::channel(None::<Result<T, EgressError>>);
                let entry = Arc::new(Entry {
                    exp: Mutex::new(None),
                    flight: Arc::new(Flight { tx }),
                });
                flights.insert(key.clone(), Arc::clone(&entry));
                (entry, true)
            }
        };
        if leader {
            return self.run_leader::<T, F, Fut>(&key, entry, f).await;
        }
        let host = host_key(url);
        match self.wait_flight::<T>(&entry).await {
            FlightWait::Value(value) => {
                self.record_dupe(&host);
                Ok(value)
            }
            FlightWait::LeaderOnly => f().await.map(|(value, _)| value),
            FlightWait::Failed(error) => Err(error),
        }
    }

    /// Runs the leader's request, publishes its result to the waiters and keeps
    /// it for the returned TTL.
    async fn run_leader<T, F, Fut>(
        &self,
        key: &str,
        entry: Arc<Entry>,
        f: F,
    ) -> Result<T, EgressError>
    where
        T: Clone + Send + Sync + 'static,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(T, Duration), EgressError>>,
    {
        let flight = flight_of::<T>(&entry)?;
        // Dropping the leader (cancellation or panic) must not leave a flight
        // without a result: wake the waiters with `Cancelled` and drop the key.
        let mut guard = LeaderGuard {
            governor: self,
            key: key.to_string(),
            entry: Arc::clone(&entry),
            flight: Some(Arc::clone(&flight)),
            armed: true,
        };
        let outcome = f().await;
        match outcome {
            Ok((value, ttl)) => {
                let exp = if ttl.is_zero() {
                    None
                } else {
                    Instant::now().checked_add(ttl)
                };
                *lock(&entry.exp) = exp;
                if exp.is_none() {
                    self.drop_flight(key, &entry);
                }
                guard.armed = false;
                let _ = flight.tx.send(Some(Ok(value.clone())));
                Ok(value)
            }
            Err(error) => {
                self.drop_flight(key, &entry);
                guard.armed = false;
                let _ = flight.tx.send(Some(Err(error.clone())));
                Err(error)
            }
        }
    }

    /// Removes `entry` from the flight map when it is still the current one.
    fn drop_flight(&self, key: &str, entry: &Arc<Entry>) {
        let mut flights = lock(&self.flights);
        let current = flights.get(key);
        if current.is_some_and(|current| Arc::ptr_eq(current, entry)) {
            flights.remove(key);
        }
    }

    /// Waits for the leader of `entry` to publish a value.
    async fn wait_flight<T>(&self, entry: &Arc<Entry>) -> FlightWait<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        let flight = match flight_of::<T>(entry) {
            Ok(flight) => flight,
            Err(error) => return FlightWait::Failed(error),
        };
        let mut receiver = flight.tx.subscribe();
        loop {
            let ready = receiver.borrow().clone();
            if let Some(result) = ready {
                return match result {
                    Ok(value) => FlightWait::Value(value),
                    Err(error) if error.is_leader_only() => FlightWait::LeaderOnly,
                    Err(error) => FlightWait::Failed(error),
                };
            }
            if receiver.changed().await.is_err() {
                return FlightWait::LeaderOnly;
            }
        }
    }

    /// Counts one coalesced caller (Go `hostState.dupes`).
    fn record_dupe(&self, host: &str) {
        if self.is_off() {
            return;
        }
        let state = self.host(host);
        lock(&state.inner).dupes += 1;
    }

    /// One noise line per active host, then starts a new minute (Go
    /// `Governor.report`).
    pub fn report(&self) -> Vec<String> {
        let mut hosts = lock(&self.hosts);
        let name = &self.profile.name;
        let mut out = Vec::new();
        for (key, state) in hosts.iter_mut() {
            let mut inner = lock(&state.inner);
            if inner.sent == 0 && inner.dupes == 0 {
                continue;
            }
            inner.peak_rpm = inner.peak_rpm.max(inner.sent);
            out.push(format!(
                "[egress] noise profile={name} host={key} rpm={} peak_rpm={} peak_inflight={} duplicates_avoided={}",
                inner.sent, inner.peak_rpm, inner.peak_inflight, inner.dupes
            ));
            inner.sent = 0;
            inner.dupes = 0;
            inner.peak_inflight = inner.inflight;
        }
        out.sort();
        out
    }
}

/// A held send slot: it frees the per-host and global concurrency slots when
/// dropped (Go `release`). The global slot is released first, then the host
/// slot, matching Go's `<-g.global; <-h.slots`.
pub struct Permit {
    _global: Option<OwnedSemaphorePermit>,
    _host: Option<OwnedSemaphorePermit>,
    state: Option<Arc<HostState>>,
}

impl Permit {
    /// The no-op permit of the `off` profile.
    fn passthrough() -> Self {
        Self {
            _host: None,
            _global: None,
            state: None,
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Some(state) = self.state.as_ref() {
            let mut inner = lock(&state.inner);
            inner.inflight = inner.inflight.saturating_sub(1);
        }
    }
}

/// The per-host rate and concurrency state (Go `hostState`).
struct HostState {
    slots: Arc<Semaphore>,
    inner: Mutex<HostInner>,
}

/// The mutable part of [`HostState`].
struct HostInner {
    tokens: f64,
    last: Instant,
    not_before: Instant,
    sent: u64,
    dupes: u64,
    inflight: u64,
    peak_inflight: u64,
    peak_rpm: u64,
}

/// One in-flight (or cached) GET (Go `flight`), type-erased in the map.
struct Flight<T> {
    tx: watch::Sender<Option<Result<T, EgressError>>>,
}

/// The flight map entry: the type-erased flight plus its cache expiry.
struct Entry {
    exp: Mutex<Option<Instant>>,
    flight: Arc<dyn Any + Send + Sync>,
}

impl Entry {
    /// Whether the cached flight has expired.
    fn expired(&self, now: Instant) -> bool {
        match *lock(&self.exp) {
            Some(exp) => now >= exp,
            None => false,
        }
    }
}

/// What a waiter learned from the leader.
enum FlightWait<T> {
    /// The leader's value.
    Value(T),
    /// The leader's failure is its own: run an uncached request.
    LeaderOnly,
    /// The leader's failure is shared.
    Failed(EgressError),
}

/// Drops the flight and wakes its waiters when the leader goes away.
struct LeaderGuard<'a, T> {
    governor: &'a Governor,
    key: String,
    entry: Arc<Entry>,
    flight: Option<Arc<Flight<T>>>,
    armed: bool,
}

impl<T> Drop for LeaderGuard<'_, T> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.governor.drop_flight(&self.key, &self.entry);
        if let Some(flight) = self.flight.take() {
            let _ = flight.tx.send(Some(Err(EgressError::Cancelled)));
        }
    }
}

/// Downcasts a type-erased flight, or reports a key type mismatch.
fn flight_of<T: Send + Sync + 'static>(entry: &Arc<Entry>) -> Result<Arc<Flight<T>>, EgressError> {
    Arc::clone(&entry.flight)
        .downcast::<Flight<T>>()
        .map_err(|_| EgressError::request("egress: cache key type mismatch"))
}

/// The cache key of one GET: namespace, task, method, URL and sorted headers
/// (Go `transport.RoundTrip`).
fn share_key(namespace: &str, task: &str, url: &Url, headers: &[(String, String)]) -> String {
    let mut sorted: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    sorted.sort_unstable();
    let mut header = String::new();
    for (name, value) in sorted {
        header.push_str(name);
        header.push_str(": ");
        header.push_str(value);
        header.push('\n');
    }
    format!("{namespace}\0{task}\0GET {url}\0{header}")
}

/// The mutex guard, recovering from a panic while the lock was held.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A small splitmix64 generator, seeded once per process.
fn next_rand() -> u64 {
    static STATE: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let mut value = STATE.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// The month number of a three-letter name, case-insensitive.
fn month_number(name: &str) -> Option<i64> {
    MONTHS
        .iter()
        .position(|month| month.eq_ignore_ascii_case(name))
        .map(|index| index as i64 + 1)
}

/// Parses `HH:MM:SS` into its parts, with range checks.
fn split_time(value: &str) -> Option<(i64, i64, i64)> {
    let mut parts = value.split(':');
    let hour = parts.next()?.parse::<i64>().ok()?;
    let minute = parts.next()?.parse::<i64>().ok()?;
    let second = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..61).contains(&second) {
        return None;
    }
    Some((hour, minute, second))
}

/// Parses an HTTP date (`Retry-After`), in IMF-fixdate, RFC 850 or asctime
/// form, as `http.ParseTime` does (Go `Governor.backoff`).
pub fn parse_http_date(value: &str) -> Option<SystemTime> {
    let value = value.trim();
    let value = value.strip_suffix("GMT")?.trim();
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let (day, month, year, hour, minute, second) = match tokens.as_slice() {
        [_, date, time] => {
            let (hour, minute, second) = split_time(time)?;
            let mut parts = date.split('-');
            let day = parts.next()?.parse::<i64>().ok()?;
            let month = month_number(parts.next()?)?;
            let short_year = parts.next()?.parse::<i64>().ok()?;
            if parts.next().is_some() {
                return None;
            }
            let year = if short_year < 70 {
                2000 + short_year
            } else {
                1900 + short_year
            };
            (day, month, year, hour, minute, second)
        }
        [_, first, second, third, fourth] => {
            if month_number(first).is_some() {
                // asctime: "Nov  6 08:49:37 1994".
                let (hour, minute, sec) = split_time(third)?;
                (
                    second.parse::<i64>().ok()?,
                    month_number(first)?,
                    fourth.parse::<i64>().ok()?,
                    hour,
                    minute,
                    sec,
                )
            } else {
                // IMF-fixdate: "06 Nov 1994 08:49:37".
                let (hour, minute, sec) = split_time(fourth)?;
                (
                    first.parse::<i64>().ok()?,
                    month_number(second)?,
                    third.parse::<i64>().ok()?,
                    hour,
                    minute,
                    sec,
                )
            }
        }
        _ => return None,
    };
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return None;
    }
    let seconds = days_from_civil(year, month, day)?
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(u64::try_from(seconds).ok()?))
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    fn open_profile() -> Profile {
        Profile {
            name: "test".to_string(),
            rps: 1000.0,
            burst: 1000,
            max_inflight_per_host: 100,
            max_inflight_global: 100,
            jitter: 0.0,
            cache_ttl: Duration::from_secs(60),
            max_retries: 2,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_secs(2),
            user_agent: String::new(),
        }
    }

    fn governor() -> Governor {
        Governor::new(open_profile())
    }

    #[test]
    fn profile_names() {
        assert!(Profile::from_name("off").is_off());
        assert!(Profile::from_name("").is_off());
        assert!(Profile::from_name("loud").is_off());
        assert_eq!(Profile::from_name("Quiet").rps, 2.0);
        assert_eq!(Profile::from_name("quiet").burst, 4);
        assert_eq!(
            Profile::from_name("quiet").cache_ttl,
            Duration::from_secs(60)
        );
        assert_eq!(Profile::from_name("balanced").rps, 5.0);
        assert_eq!(Profile::from_name("balanced").max_inflight_per_host, 2);
        assert_eq!(
            Profile::from_name("balanced").cache_ttl,
            Duration::from_secs(30)
        );
        assert_eq!(Profile::from_name("aggressive").rps, 20.0);
        assert_eq!(Profile::from_name("aggressive").max_inflight_per_host, 4);
        assert_eq!(
            Profile::from_name("aggressive").cache_ttl,
            Duration::from_secs(10)
        );
        assert!(Profile::default().is_off());
    }

    #[test]
    fn profile_from_env() {
        static ENV: Mutex<()> = Mutex::new(());
        let _guard = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        std::env::set_var("ARTEX_NOISE_PROFILE", "Balanced");
        std::env::set_var("ARTEX_TARGET_USER_AGENT", "x/1");
        let profile = Profile::from_env();
        assert_eq!(profile.name, "balanced");
        assert_eq!(profile.rps, 5.0);
        assert_eq!(profile.user_agent, "x/1");

        std::env::set_var("ARTEX_NOISE_PROFILE", "loud");
        assert!(Profile::from_env().is_off());

        std::env::set_var("ARTEX_NOISE_PROFILE", "aggressive");
        std::env::set_var("ARTEX_TARGET_USER_AGENT", "bad\nua");
        assert_eq!(Profile::from_env().user_agent, "");

        std::env::remove_var("ARTEX_NOISE_PROFILE");
        std::env::remove_var("ARTEX_TARGET_USER_AGENT");
        let profile = Profile::from_env();
        assert!(profile.is_off());
        assert_eq!(profile.name, "off");
    }

    #[test]
    fn host_keys() {
        assert_eq!(
            host_key(&Url::parse("http://Example.COM:80/x").expect("url")),
            "example.com"
        );
        assert_eq!(
            host_key(&Url::parse("https://a.test:8443/").expect("url")),
            "a.test:8443"
        );
        assert_eq!(
            host_key(&Url::parse("https://a.test:443/").expect("url")),
            "a.test"
        );
    }

    #[tokio::test]
    async fn token_bucket_paces_after_burst() {
        let profile = Profile {
            name: "test".to_string(),
            rps: 20.0,
            burst: 2,
            max_inflight_per_host: 8,
            max_inflight_global: 8,
            jitter: 0.0,
            cache_ttl: Duration::ZERO,
            max_retries: 0,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_secs(2),
            user_agent: String::new(),
        };
        let governor = Governor::new(profile);
        let start = Instant::now();
        for _ in 0..5 {
            let permit = governor.acquire("example.com").await.expect("permit");
            drop(permit);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(120),
            "five requests at 20 rps after a burst of two took {elapsed:?}"
        );
        assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
    }

    #[tokio::test]
    async fn rapid_concurrent_acquire_is_throttled() {
        let profile = Profile {
            rps: 20.0,
            burst: 2,
            ..open_profile()
        };
        let governor = Arc::new(Governor::new(profile));
        let start = Instant::now();
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let governor = Arc::clone(&governor);
            tasks.push(tokio::spawn(async move {
                let permit = governor.acquire("example.com").await.expect("permit");
                drop(permit);
            }));
        }
        for task in tasks {
            task.await.expect("task");
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(150),
            "six requests at 20 rps after a burst of two took {elapsed:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn in_flight_cap_holds() {
        let profile = Profile {
            max_inflight_per_host: 2,
            max_inflight_global: 8,
            ..open_profile()
        };
        let governor = Arc::new(Governor::new(profile));
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let governor = Arc::clone(&governor);
            let current = Arc::clone(&current);
            let peak = Arc::clone(&peak);
            tasks.push(tokio::spawn(async move {
                let _permit = governor.acquire("example.com").await.expect("permit");
                let now = current.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                peak.fetch_max(now, AtomicOrdering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                current.fetch_sub(1, AtomicOrdering::SeqCst);
            }));
        }
        for task in tasks {
            task.await.expect("task");
        }
        assert_eq!(peak.load(AtomicOrdering::SeqCst), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn global_in_flight_cap_holds() {
        let profile = Profile {
            max_inflight_per_host: 100,
            max_inflight_global: 2,
            ..open_profile()
        };
        let governor = Arc::new(Governor::new(profile));
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for host in ["a.test", "b.test", "c.test"] {
            let governor = Arc::clone(&governor);
            let current = Arc::clone(&current);
            let peak = Arc::clone(&peak);
            tasks.push(tokio::spawn(async move {
                let _permit = governor.acquire(host).await.expect("permit");
                let now = current.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                peak.fetch_max(now, AtomicOrdering::SeqCst);
                tokio::time::sleep(Duration::from_millis(40)).await;
                current.fetch_sub(1, AtomicOrdering::SeqCst);
            }));
        }
        for task in tasks {
            task.await.expect("task");
        }
        assert_eq!(peak.load(AtomicOrdering::SeqCst), 2);
    }

    #[test]
    fn jitter_is_bounded() {
        for (profile, want) in [
            (Profile::quiet(), Duration::from_millis(250)),
            (Profile::balanced(), Duration::from_millis(40)),
            (Profile::aggressive(), Duration::from_millis(5)),
        ] {
            let governor = Governor::new(profile);
            assert_eq!(governor.jitter_bound(), want);
            let mut nonzero = false;
            for _ in 0..1000 {
                let jitter = governor.jitter();
                assert!(jitter <= want, "jitter {jitter:?} above {want:?}");
                nonzero |= !jitter.is_zero();
            }
            assert!(nonzero, "jitter always zero");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn get_coalesces_and_caches() {
        let profile = Profile {
            cache_ttl: Duration::from_millis(200),
            ..open_profile()
        };
        let governor = Arc::new(Governor::new(profile));
        let url = Url::parse("https://example.test/x").expect("url");
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(tokio::sync::Barrier::new(5));

        let mut tasks = Vec::new();
        for _ in 0..5 {
            let governor = Arc::clone(&governor);
            let calls = Arc::clone(&calls);
            let barrier = Arc::clone(&barrier);
            let url = url.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                let calls = Arc::clone(&calls);
                governor
                    .do_get("test", "t1", &url, &[], move || {
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.fetch_add(1, AtomicOrdering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(40)).await;
                            Ok::<_, EgressError>(("ok".to_string(), Duration::from_millis(200)))
                        }
                    })
                    .await
            }));
        }
        for task in tasks {
            let value = task.await.expect("task").expect("value");
            assert_eq!(value, "ok");
        }
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);

        let cached = governor
            .do_get("test", "t1", &url, &[], || async {
                calls.fetch_add(1, AtomicOrdering::SeqCst);
                Ok::<_, EgressError>(("ok".to_string(), Duration::from_millis(200)))
            })
            .await
            .expect("cached value");
        assert_eq!(cached, "ok");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);

        let _ = governor
            .do_get("test", "t2", &url, &[], || async {
                calls.fetch_add(1, AtomicOrdering::SeqCst);
                Ok::<_, EgressError>(("ok".to_string(), Duration::from_millis(200)))
            })
            .await
            .expect("other task");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);

        let _ = governor
            .do_get("other", "t1", &url, &[], || async {
                calls.fetch_add(1, AtomicOrdering::SeqCst);
                Ok::<_, EgressError>(("ok".to_string(), Duration::from_millis(200)))
            })
            .await
            .expect("other namespace");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 3);

        tokio::time::sleep(Duration::from_millis(250)).await;
        let _ = governor
            .do_get("test", "t1", &url, &[], || async {
                calls.fetch_add(1, AtomicOrdering::SeqCst);
                Ok::<_, EgressError>(("ok".to_string(), Duration::from_millis(200)))
            })
            .await
            .expect("expired");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 4);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn get_does_not_share_failures_of_the_leader() {
        let governor = Arc::new(Governor::new(open_profile()));
        let url = Url::parse("https://example.test/x").expect("url");
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let governor = Arc::clone(&governor);
            let calls = Arc::clone(&calls);
            let barrier = Arc::clone(&barrier);
            let url = url.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                let calls = Arc::clone(&calls);
                governor
                    .do_get("test", "t1", &url, &[], move || {
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.fetch_add(1, AtomicOrdering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(30)).await;
                            Err::<(String, Duration), _>(EgressError::NotShared)
                        }
                    })
                    .await
            }));
        }
        for task in tasks {
            assert!(task.await.expect("task").is_err());
        }
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);
    }

    #[test]
    fn retry_after_parsing() {
        let governor = governor();
        assert_eq!(
            governor.retry_after(Some("1"), 0),
            RetryAfter {
                delay: Duration::from_secs(1),
                retry: true,
            }
        );
        assert_eq!(
            governor.retry_after(Some("3600"), 0),
            RetryAfter {
                delay: Duration::from_secs(3600),
                retry: false,
            }
        );
        let (delay, retry) = {
            let value = governor.retry_after(Some("not a date"), 0);
            (value.delay, value.retry)
        };
        assert_eq!(delay, Duration::from_millis(10));
        assert!(retry);
        let value = governor.retry_after(None, 1);
        assert_eq!(value.delay, Duration::from_millis(20));
        assert!(value.retry);
        let value = governor.retry_after(None, 20);
        assert_eq!(value.delay, Duration::from_secs(2));
        assert!(value.retry);
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(784_111_777))
        );
        let future = fmt_http_date(SystemTime::now() + Duration::from_secs(2));
        let value = governor.retry_after(Some(&future), 0);
        assert!(value.retry);
        assert!(
            value.delay > Duration::from_millis(500) && value.delay <= Duration::from_secs(2),
            "HTTP-date Retry-After: {:?}",
            value.delay
        );
    }

    #[test]
    fn retry_rules() {
        let governor = governor();
        assert!(governor.should_retry("GET", 429, 0));
        assert!(governor.should_retry("HEAD", 503, 0));
        assert!(governor.should_retry("OPTIONS", 429, 1));
        assert!(!governor.should_retry("POST", 429, 0));
        assert!(!governor.should_retry("GET", 500, 0));
        assert!(!governor.should_retry("GET", 429, 2));
    }

    #[tokio::test]
    async fn off_is_passthrough() {
        let governor = Governor::new(Profile::off());
        let start = Instant::now();
        for _ in 0..100 {
            let permit = governor.acquire("example.com").await.expect("permit");
            drop(permit);
        }
        assert!(start.elapsed() < Duration::from_millis(500));
        assert!(governor.report().is_empty());
        let value = governor
            .do_get(
                "test",
                "t1",
                &Url::parse("https://example.test/").expect("url"),
                &[],
                || async { Ok::<_, EgressError>(("ok".to_string(), Duration::ZERO)) },
            )
            .await
            .expect("passthrough");
        assert_eq!(value, "ok");
        assert!(governor.report().is_empty());
    }

    #[tokio::test]
    async fn report_counts_sent_and_dupes() {
        let governor = Governor::new(open_profile());
        let permit = governor.acquire("example.com").await.expect("permit");
        drop(permit);
        governor.record_dupe("example.com");
        let report = governor.report();
        assert_eq!(report.len(), 1);
        assert!(report[0].contains("rpm=1 "), "{}", report[0]);
        assert!(report[0].contains("duplicates_avoided=1"), "{}", report[0]);
        assert!(governor.report().is_empty());
    }

    /// Formats a `SystemTime` as an IMF-fixdate, for the `Retry-After` tests.
    fn fmt_http_date(time: SystemTime) -> String {
        const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let seconds = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let days = seconds.div_euclid(86_400);
        let time_of_day = seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let weekday = (days + 4).rem_euclid(7) as usize;
        format!(
            "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
            WEEKDAYS[weekday],
            day,
            MONTHS[(month - 1) as usize],
            year,
            time_of_day / 3_600,
            (time_of_day / 60) % 60,
            time_of_day % 60
        )
    }

    /// The inverse of [`days_from_civil`], for the test date formatter.
    fn civil_from_days(days: i64) -> (i64, i64, i64) {
        let days = days + 719_468;
        let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_prime = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
        let month = if month_prime < 10 {
            month_prime + 3
        } else {
            month_prime - 9
        };
        (if month <= 2 { year + 1 } else { year }, month, day)
    }
}
