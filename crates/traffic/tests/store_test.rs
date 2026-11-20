//! Integration test for the traffic store: open a temp dir, record exchanges,
//! search, get, hosts, count, ingest_external, delete_host, cleanup.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use traffic::{Exchange, ExternalExchange, TrafficError, TrafficStore};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "traffic-store-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn proxy_exchange(method: &str, url: &str, status: u16, req: &str, resp: &str) -> Exchange {
    Exchange {
        method: method.to_string(),
        url: url.to_string(),
        status,
        request_headers: BTreeMap::from([("content-type".to_string(), "text/plain".to_string())]),
        request_body: req.as_bytes().to_vec(),
        response_headers: BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
        response_body: resp.as_bytes().to_vec(),
        host: String::new(),
        path: String::new(),
        timestamp: 1_700_000_000,
    }
}

#[test]
fn store_full_lifecycle() {
    let tmp = TempDir::new("lifecycle");
    let store = TrafficStore::open(tmp.0.clone()).expect("open store");
    assert!(tmp.0.join("_index").join("index.sqlite").exists());
    assert!(tmp.0.join("blobs").exists());

    // record two proxy-captured exchanges
    let id1 = store
        .record(proxy_exchange(
            "GET",
            "https://api.example.com/v1/users/42",
            200,
            "",
            "user-listing",
        ))
        .expect("record 1");
    let id2 = store
        .record(proxy_exchange(
            "POST",
            "https://api.example.com/v1/login",
            401,
            "user=admin&password=hunter2",
            "unauthorized",
        ))
        .expect("record 2");

    assert_eq!(store.count().expect("count"), 2);

    // get round-trips bodies and templated path
    let ex = store.get(&id1).expect("get 1");
    assert_eq!(ex.method, "GET");
    assert_eq!(ex.status, 200);
    assert_eq!(ex.response_body, b"user-listing");
    assert_eq!(ex.path, "/v1/users/{id}");
    assert_eq!(ex.host, "api.example.com");

    // hosts: distinct host with count
    let hosts = store.hosts().expect("hosts");
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].host, "api.example.com");
    assert_eq!(hosts[0].count, 2);

    // search: FTS finds body content of the login exchange
    let hits = store.search("hunter2", 10, 0).expect("search body");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id2);
    assert_eq!(hits[0].status, 401);

    // search: metadata term
    let hits = store.search("login", 10, 0).expect("search meta");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id2);

    // paging
    let page0 = store.search("", 1, 0).expect("page0");
    let page1 = store.search("", 1, 1).expect("page1");
    assert_eq!(page0.len(), 1);
    assert_eq!(page1.len(), 1);
    assert_ne!(page0[0].id, page1[0].id);

    // ingest_external
    let ext = ExternalExchange {
        method: "PUT".to_string(),
        url: "https://api.example.com/v1/items/7".to_string(),
        status: 201,
        request_headers: BTreeMap::from([(
            "content-type".to_string(),
            "application/json".to_string(),
        )]),
        request_body: "{\"name\":\"widget\"}".to_string(),
        response_headers: BTreeMap::from([(
            "content-type".to_string(),
            "application/json".to_string(),
        )]),
        response_body: "{\"id\":7}".to_string(),
    };
    let (ingested, failed) = store.ingest_external(vec![ext]).expect("ingest");
    assert_eq!((ingested, failed), (1, 0));
    assert_eq!(store.count().expect("count after ingest"), 3);

    // spilled body -> blob store round-trip
    let big = "Z".repeat(512 * 1024);
    let ex = proxy_exchange("GET", "https://api.example.com/big", 200, "", &big);
    let id_big = store.record(ex).expect("record big");
    let got = store.get(&id_big).expect("get big");
    assert_eq!(got.response_body.len(), big.len());
    assert_eq!(got.response_body, big.as_bytes());
    assert!(tmp
        .0
        .join("blobs")
        .read_dir()
        .map(|mut d| d.next().is_some())
        .unwrap_or(false));

    // delete_host removes the example.com rows + garbage-collects blobs
    let removed = store.delete_host("example.com").expect("delete host");
    assert!(removed >= 3);
    assert_eq!(store.count().expect("count after delete"), 0);
    assert!(store.hosts().expect("hosts after delete").is_empty());

    // get on a deleted id errors
    assert!(matches!(store.get(&id1), Err(TrafficError::Invalid(_))));

    // store is empty now; delete_all is a no-op success
    let deleted = store.delete_all().expect("delete all");
    assert_eq!(deleted, 0);

    // temp dir is removed on drop
    drop(store);
    drop(tmp);
}

#[test]
fn record_then_delete_all_reclaims() {
    let tmp = TempDir::new("deleteall");
    let store = TrafficStore::open(tmp.0.clone()).expect("open");
    for i in 0..5 {
        store
            .record(proxy_exchange(
                "GET",
                &format!("https://h{i}.example.com/x"),
                200,
                "",
                "body",
            ))
            .expect("record");
    }
    assert_eq!(store.count().expect("count"), 5);
    let n = store.delete_all().expect("delete all");
    assert_eq!(n, 5);
    assert_eq!(store.count().expect("count after"), 0);
    assert!(store.search("", 10, 0).expect("search").is_empty());
}
