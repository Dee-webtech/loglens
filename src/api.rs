use crate::db::Db;
use crate::ingest::{parse_payload, Engine};
use crate::metrics::Metrics;
use crate::{auth, now_ms, query, search, Error};
use axum::extract::{DefaultBodyLimit, Query as Params, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub db: Arc<Db>,
    pub metrics: Arc<Metrics>,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let code = match &self {
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Busy => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if code == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
        }
        (code, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .route("/api/v1/logs", post(ingest_h))
        .route("/api/v1/search", get(search_h))
        .route("/api/v1/top", get(top_h))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state)
}

fn authenticate(s: &AppState, h: &HeaderMap) -> Result<i64, Error> {
    let key = auth::bearer(h).ok_or(Error::Unauthorized)?;
    s.db.find_by_hash(&auth::hash_key(key))?
        .ok_or(Error::Unauthorized)
}

fn num(p: &HashMap<String, String>, k: &str, default: u64) -> Result<u64, Error> {
    match p.get(k) {
        None => Ok(default),
        Some(v) => v
            .parse()
            .map_err(|_| Error::BadRequest(format!("invalid {k}"))),
    }
}

async fn index() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn metrics(State(s): State<AppState>) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        s.metrics.render(),
    )
}

async fn ingest_h(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<Value>, Error> {
    let tenant = authenticate(&s, &headers)?;
    let docs = parse_payload(&body, now_ms())?;
    let n = docs.len();
    let rx = s.engine.enqueue(tenant, docs)?;
    rx.await.map_err(|_| Error::Io("writer stopped".into()))??;
    tracing::info!(tenant, accepted = n, "ingest");
    Ok(Json(json!({ "accepted": n })))
}

async fn search_h(
    State(s): State<AppState>,
    headers: HeaderMap,
    Params(p): Params<HashMap<String, String>>,
) -> Result<Json<Value>, Error> {
    let tenant = authenticate(&s, &headers)?;
    let q = query::parse(p.get("q").map(String::as_str).unwrap_or(""));
    let from = num(&p, "from", 0)?;
    let to = num(&p, "to", u64::MAX)?;
    let limit = num(&p, "limit", 100)?.clamp(1, 1000) as usize;
    let t = s.engine.tenant(tenant)?;
    let hits = search::search(&t, &q, from, to, limit, &s.metrics, true);
    Ok(Json(json!({ "count": hits.len(), "hits": hits })))
}

async fn top_h(
    State(s): State<AppState>,
    headers: HeaderMap,
    Params(p): Params<HashMap<String, String>>,
) -> Result<Json<Value>, Error> {
    let tenant = authenticate(&s, &headers)?;
    let k = num(&p, "k", 10)?.clamp(1, 100) as usize;
    let level = p.get("level").map(String::as_str).filter(|l| !l.is_empty());
    let t = s.engine.tenant(tenant)?;
    let rows: Vec<Value> = search::top_templates(&t, k, level)
        .into_iter()
        .map(|(template, count)| json!({ "template": template, "count": count }))
        .collect();
    Ok(Json(json!({ "templates": rows })))
}
