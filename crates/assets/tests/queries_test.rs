//! Integration test: asset & company queries against a real Postgres. Requires
//! ARTEX_PG_DSN; skips when Postgres is unreachable.

use assets::{AssetStore, CompanyStore};
use database::open;
use serde_json::Value;

fn dsn() -> Option<String> {
    std::env::var("ARTEX_PG_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| Some("postgres://artex:artex@127.0.0.1:55432/artex?sslmode=disable".into()))
}

#[tokio::test]
async fn companies_and_assets_queries() {
    let Some(dsn) = dsn() else { return };
    let db = match open(&dsn).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("skipping: postgres unavailable ({e})");
            return;
        }
    };

    let companies = CompanyStore::new(db.clone());
    let assets = AssetStore::new(db.clone());

    // Clean any prior test data so the test is idempotent.
    sqlx::query("DELETE FROM assets WHERE root_domain IN ('acme.com', 'globex.com')")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM companies WHERE nkey IN ('acme', 'globex')")
        .execute(db.pool())
        .await
        .unwrap();

    // Create two companies.
    let (id1, c1) = companies.upsert("Acme", "").await.unwrap();
    let (id2, _) = companies.upsert("Globex", "").await.unwrap();
    assert!(c1, "first upsert creates");
    let (_id1b, c1b) = companies.upsert("Acme", "").await.unwrap();
    assert!(!c1b, "second upsert of same name updates");
    assert_eq!(id1, _id1b);

    // Insert a root_domain asset per company.
    for (cid, name) in [(id1, "acme.com"), (id2, "globex.com")] {
        sqlx::query(
            "INSERT INTO assets(type, company_id, task_ids, root_domain, domain) \
             VALUES ('root_domain', $1, ARRAY[1]::bigint[], $2, $2)",
        )
        .bind(cid)
        .bind(name)
        .execute(db.pool())
        .await
        .unwrap();
    }

    // counts_by_type.
    let counts = assets.counts_by_type().await.unwrap();
    assert_eq!(counts.get("root_domain").copied(), Some(2));

    // query_by_type returns both, newest first.
    let list = assets.query_by_type("root_domain", 50, 0).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].root_domain.as_deref(), Some("globex.com"));

    // company_id filter count.
    assert_eq!(assets.count_by_company(id1, None).await.unwrap(), 1);
    assert_eq!(
        assets
            .count_by_company(id1, Some("root_domain"))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        assets
            .count_by_company(id1, Some("subdomain"))
            .await
            .unwrap(),
        0
    );

    // list companies with scope + asset_count.
    let list = companies.list().await.unwrap();
    let acme = list.iter().find(|c| c.id == id1).unwrap();
    assert_eq!(acme.name, "Acme");
    assert_eq!(acme.asset_count, 1);
    assert!(acme.scope.is_empty());

    // Serialize an asset to JSON (API shape).
    let v = serde_json::to_value(&list_assets_first(&assets).await).unwrap();
    assert!(v.get("id").is_some());
    assert_eq!(v["type"], "root_domain");
}

async fn list_assets_first(assets: &AssetStore) -> Value {
    let list = assets.query_by_type("root_domain", 1, 0).await.unwrap();
    serde_json::to_value(&list[0]).unwrap()
}
