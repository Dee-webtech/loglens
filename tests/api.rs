use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use loglens::api::{router, AppState};
use loglens::auth::hash_key;
use loglens::config::Config;
use loglens::db::Db;
use loglens::ingest::{gen_logs, Engine, Msg};
use loglens::metrics::Metrics;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower::ServiceExt;

struct H {
    app: Router,
    db: Arc<Db>,
    engine: Arc<Engine>,
    _rx: Option<mpsc::Receiver<Msg>>,
    _dir: tempfile::TempDir,
}

fn harness(cap: usize, writer: bool) -> H {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::new(dir.path().to_path_buf());
    cfg.queue_cap = cap;
    let metrics = Arc::new(Metrics::default());
    let db = Arc::new(Db::open(&dir.path().join("meta.db")).unwrap());
    let (engine, rx) = Engine::new(cfg, metrics.clone()).unwrap();
    let rx = if writer {
        let e = engine.clone();
        std::thread::spawn(move || e.run(rx));
        None
    } else {
        Some(rx)
    };
    let app = router(AppState {
        engine: engine.clone(),
        db: db.clone(),
        metrics,
    });
    H {
        app,
        db,
        engine,
        _rx: rx,
        _dir: dir,
    }
}

fn new_tenant(h: &H, name: &str) -> (String, i64) {
    let key = h.db.create_tenant(name).unwrap();
    let id = h.db.find_by_hash(&hash_key(&key)).unwrap().unwrap();
    (key, id)
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    key: Option<&str>,
    body: &str,
) -> (StatusCode, String) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(k) = key {
        b = b.header("authorization", format!("Bearer {k}"));
    }
    let req = b.body(Body::from(body.to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn hits(h: &H, key: &str, uri: &str) -> Vec<Value> {
    let (st, body) = call(&h.app, "GET", uri, Some(key), "").await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    v["hits"].as_array().unwrap().clone()
}

fn stamps(hits: &[Value]) -> Vec<u64> {
    hits.iter().map(|x| x["ts"].as_u64().unwrap()).collect()
}

#[tokio::test]
async fn rejects_missing_and_bad_keys() {
    let h = harness(8, true);
    for (m, u) in [
        ("GET", "/api/v1/search?q=x"),
        ("POST", "/api/v1/logs"),
        ("GET", "/api/v1/top"),
    ] {
        assert_eq!(
            call(&h.app, m, u, None, "").await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&h.app, m, u, Some("nope"), "").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(&h.app, "GET", "/healthz", None, "").await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn tenants_cannot_see_each_other() {
    let h = harness(8, true);
    let (ka, _) = new_tenant(&h, "a");
    let (kb, _) = new_tenant(&h, "b");
    let line = r#"{"level":"info","service":"x","message":"alpha secret"}"#;
    assert_eq!(
        call(&h.app, "POST", "/api/v1/logs", Some(&ka), line)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(hits(&h, &ka, "/api/v1/search?q=secret").await.len(), 1);
    assert_eq!(hits(&h, &kb, "/api/v1/search?q=secret").await.len(), 0);
    assert_eq!(hits(&h, &kb, "/api/v1/search").await.len(), 0);
}

#[tokio::test]
async fn ingest_then_search() {
    let h = harness(8, true);
    let (key, _) = new_tenant(&h, "acme");
    let body = r#"
{"ts":1000,"level":"error","service":"api","message":"db timeout after 30s"}
{"ts":2000,"level":"info","service":"api","message":"db timeout recovered"}
{"ts":3000,"level":"error","service":"web","message":"db timeout retry 3"}
"#;
    let (st, resp) = call(&h.app, "POST", "/api/v1/logs", Some(&key), body).await;
    assert_eq!(st, StatusCode::OK);
    assert!(resp.contains("\"accepted\":3"));

    let all = hits(&h, &key, "/api/v1/search?q=timeout").await;
    assert_eq!(stamps(&all), vec![3000, 2000, 1000]);
    let excl = hits(&h, &key, "/api/v1/search?q=timeout%20-retry").await;
    assert_eq!(stamps(&excl), vec![2000, 1000]);
    let errs = hits(&h, &key, "/api/v1/search?q=timeout%20level:error").await;
    assert_eq!(stamps(&errs), vec![3000, 1000]);
    let ranged = hits(&h, &key, "/api/v1/search?q=timeout&from=1500&to=2500").await;
    assert_eq!(stamps(&ranged), vec![2000]);
    assert_eq!(
        hits(&h, &key, "/api/v1/search?q=timeout&limit=1")
            .await
            .len(),
        1
    );

    let (st, top) = call(&h.app, "GET", "/api/v1/top?k=5&level=error", Some(&key), "").await;
    assert_eq!(st, StatusCode::OK);
    let v: Value = serde_json::from_str(&top).unwrap();
    assert_eq!(v["templates"].as_array().unwrap().len(), 2);

    let arr = r#"[{"level":"info","service":"web","message":"array form ok"}]"#;
    assert_eq!(
        call(&h.app, "POST", "/api/v1/logs", Some(&key), arr)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(hits(&h, &key, "/api/v1/search?q=array").await.len(), 1);

    let (st, err) = call(&h.app, "POST", "/api/v1/logs", Some(&key), "{nope").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(err.contains("\"error\""));
}

#[tokio::test]
async fn full_queue_returns_429() {
    let h = harness(1, false);
    let (key, id) = new_tenant(&h, "acme");
    let _pending = h.engine.enqueue(id, gen_logs(1, 1, 1)).unwrap();
    let line = r#"{"level":"info","service":"x","message":"m"}"#;
    let (st, body) = call(&h.app, "POST", "/api/v1/logs", Some(&key), line).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
    assert!(body.contains("queue full"));
}
