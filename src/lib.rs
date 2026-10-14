pub mod api;
pub mod auth;
pub mod bloom;
pub mod config;
pub mod db;
pub mod ingest;
pub mod memtable;
pub mod metrics;
pub mod postings;
pub mod query;
pub mod search;
pub mod segment;
pub mod top;
pub mod varint;
pub mod wal;

use serde::Serialize;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Doc {
    pub ts: u64,
    pub level: String,
    pub service: String,
    pub message: String,
}

#[derive(Debug)]
pub enum Error {
    Io(String),
    Corrupt(String),
    BadRequest(String),
    Unauthorized,
    Busy,
    Db(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(m) => write!(f, "io error: {m}"),
            Error::Corrupt(m) => write!(f, "corrupt data: {m}"),
            Error::BadRequest(m) => write!(f, "bad request: {m}"),
            Error::Unauthorized => write!(f, "missing or invalid API key"),
            Error::Busy => write!(f, "ingest queue full, retry later"),
            Error::Db(m) => write!(f, "database error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Error {
        Error::Db(e.to_string())
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}