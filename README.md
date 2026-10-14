# LogLens
![CI](https://github.com/Dee-webtech/loglens/actions/workflows/ci.yml/badge.svg)

Lightweight multi-tenant log ingestion and search in Rust: WAL, immutable segments, delta+varint postings and Bloom pruning, no external search engine.

## Features
- Per-tenant API keys (only SHA-256 hashes stored in SQLite), strict tenant isolation
- NDJSON or JSON-array ingest, bounded queue (429 when full), WAL, checksummed segments
- Search: ANDed terms, `-exclude`, `level:error`, time range, newest first; `/api/v1/top` templates
- `/metrics`, `/healthz`, JSON errors, JSON tracing logs, one-page UI at `/`

## Architecture
```
client -> axum -> bounded queue -> writer -> WAL -> memtable -> sealed segment files
search: time range -> Bloom filter -> postings intersect/difference -> newest first
```

## Quick start
```bash
cargo build --release
./target/release/loglens create-tenant acme   # prints the key once
export KEY=ll_...
./target/release/loglens serve                # http://127.0.0.1:8080 (keep running)
./target/release/loglens gen-logs --count 5000 | curl -s -X POST localhost:8080/api/v1/logs -H "Authorization: Bearer $KEY" --data-binary @-   # 2nd terminal
curl -s -H "Authorization: Bearer $KEY" "localhost:8080/api/v1/search?q=timeout%20level:error&limit=5"
curl -s -H "Authorization: Bearer $KEY" "localhost:8080/api/v1/top?k=5&level=error"
```

## Test and bench
`cargo test` runs unit and API tests; `cargo bench` runs criterion (100k lines, with and without pruning).

## Limitations
Single writer thread; segments are loaded fully into memory; a crash between the segment write and WAL reset can actually duplicate documents; no TLS.

## Roadmap
Compaction, retention and rate limiting (not included yet).

## Git workflow
Conventional Commits; feature/* -> develop -> testing -> staging -> main; CI runs on all four.