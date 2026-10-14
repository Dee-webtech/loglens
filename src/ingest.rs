use crate::config::Config;
use crate::memtable::Memtable;
use crate::metrics::{add, Metrics};
use crate::segment::Segment;
use crate::wal::Wal;
use crate::{Doc, Error};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tokio::sync::{mpsc, oneshot};

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn rlock<T>(m: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    m.read().unwrap_or_else(|e| e.into_inner())
}

pub fn wlock<T>(m: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    m.write().unwrap_or_else(|e| e.into_inner())
}

pub struct State {
    pub mem: Memtable,
    pub segs: Vec<Arc<Segment>>,
    next_id: u64,
}

pub struct Tenant {
    dir: PathBuf,
    pub state: RwLock<State>,
    wal: Mutex<Wal>,
}

impl Tenant {
    pub fn open(dir: &Path) -> Result<Tenant, Error> {
        fs::create_dir_all(dir)?;
        let mut found: Vec<(u64, PathBuf)> = Vec::new();
        for e in fs::read_dir(dir)? {
            let p = e?.path();
            let id = p
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("seg-"))
                .and_then(|n| n.strip_suffix(".seg"))
                .and_then(|n| n.parse::<u64>().ok());
            if let Some(id) = id {
                found.push((id, p));
            }
        }
        found.sort();
        let mut segs = Vec::new();
        let mut next_id = 1;
        for (id, p) in found {
            segs.push(Arc::new(Segment::load(id, &p)?));
            next_id = id + 1;
        }
        let (wal, replayed) = Wal::open(&dir.join("wal.log"))?;
        let mut mem = Memtable::new();
        for d in replayed {
            mem.insert(d);
        }
        Ok(Tenant {
            dir: dir.to_path_buf(),
            state: RwLock::new(State { mem, segs, next_id }),
            wal: Mutex::new(wal),
        })
    }

    pub fn append(&self, docs: Vec<Doc>, seal_bytes: usize) -> Result<(), Error> {
        lock(&self.wal).append(&docs)?;
        let mut st = wlock(&self.state);
        for d in docs {
            st.mem.insert(d);
        }
        if st.mem.bytes() >= seal_bytes {
            self.seal_locked(&mut st)?;
        }
        Ok(())
    }

    pub fn seal(&self) -> Result<(), Error> {
        let mut st = wlock(&self.state);
        self.seal_locked(&mut st)
    }

    fn seal_locked(&self, st: &mut State) -> Result<(), Error> {
        if st.mem.is_empty() {
            return Ok(());
        }
        let id = st.next_id;
        let seg = Segment::build(id, st.mem.docs().to_vec());
        seg.write(&self.dir.join(format!("seg-{id:08}.seg")))?;
        lock(&self.wal).reset()?;
        st.segs.push(Arc::new(seg));
        st.next_id += 1;
        st.mem = Memtable::new();
        Ok(())
    }
}

pub enum Msg {
    Write {
        tenant: i64,
        docs: Vec<Doc>,
        ack: oneshot::Sender<Result<(), Error>>,
    },
    Flush(oneshot::Sender<()>),
}

pub struct Engine {
    pub cfg: Config,
    metrics: Arc<Metrics>,
    tenants: RwLock<HashMap<i64, Arc<Tenant>>>,
    tx: mpsc::Sender<Msg>,
}

impl Engine {
    pub fn new(
        cfg: Config,
        metrics: Arc<Metrics>,
    ) -> Result<(Arc<Engine>, mpsc::Receiver<Msg>), Error> {
        let root = cfg.data_dir.join("tenants");
        let mut map = HashMap::new();
        if root.exists() {
            for e in fs::read_dir(&root)? {
                let e = e?;
                let id = e.file_name().to_str().and_then(|s| s.parse::<i64>().ok());
                if let Some(id) = id {
                    map.insert(id, Arc::new(Tenant::open(&e.path())?));
                }
            }
        }
        let (tx, rx) = mpsc::channel(cfg.queue_cap.max(1));
        let engine = Engine {
            cfg,
            metrics,
            tenants: RwLock::new(map),
            tx,
        };
        Ok((Arc::new(engine), rx))
    }

    pub fn tenant(&self, id: i64) -> Result<Arc<Tenant>, Error> {
        if let Some(t) = rlock(&self.tenants).get(&id) {
            return Ok(t.clone());
        }
        let mut map = wlock(&self.tenants);
        if let Some(t) = map.get(&id) {
            return Ok(t.clone());
        }
        let dir = self.cfg.data_dir.join("tenants").join(id.to_string());
        let t = Arc::new(Tenant::open(&dir)?);
        map.insert(id, t.clone());
        Ok(t)
    }

    pub fn enqueue(
        &self,
        tenant: i64,
        docs: Vec<Doc>,
    ) -> Result<oneshot::Receiver<Result<(), Error>>, Error> {
        let (ack, rx) = oneshot::channel();
        self.tx
            .try_send(Msg::Write { tenant, docs, ack })
            .map_err(|_| Error::Busy)?;
        Ok(rx)
    }

    pub async fn flush(&self) {
        let (ack, rx) = oneshot::channel();
        if self.tx.send(Msg::Flush(ack)).await.is_ok() {
            let _ = rx.await;
        }
    }

    pub fn run(self: Arc<Self>, mut rx: mpsc::Receiver<Msg>) {
        while let Some(msg) = rx.blocking_recv() {
            match msg {
                Msg::Write { tenant, docs, ack } => {
                    let n = docs.len() as u64;
                    let r = self
                        .tenant(tenant)
                        .and_then(|t| t.append(docs, self.cfg.seal_bytes));
                    if r.is_ok() {
                        add(&self.metrics.ingested, n);
                    }
                    let _ = ack.send(r);
                }
                Msg::Flush(ack) => {
                    let all: Vec<Arc<Tenant>> = rlock(&self.tenants).values().cloned().collect();
                    for t in all {
                        if let Err(e) = t.seal() {
                            tracing::error!(error = %e, "seal failed");
                        }
                    }
                    let _ = ack.send(());
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct RawLog {
    ts: Option<u64>,
    level: String,
    service: String,
    message: String,
}

pub fn parse_payload(body: &str, now: u64) -> Result<Vec<Doc>, Error> {
    let t = body.trim();
    let raws: Vec<RawLog> = if t.starts_with('[') {
        serde_json::from_str(t)
            .map_err(|e| Error::BadRequest(format!("invalid JSON array: {e}")))?
    } else {
        let mut v: Vec<RawLog> = Vec::new();
        for (i, line) in t.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let raw = serde_json::from_str(line)
                .map_err(|e| Error::BadRequest(format!("line {}: {e}", i + 1)))?;
            v.push(raw);
        }
        v
    };
    if raws.is_empty() {
        return Err(Error::BadRequest("no log lines".into()));
    }
    Ok(raws
        .into_iter()
        .map(|r| Doc {
            ts: r.ts.unwrap_or(now),
            level: r.level,
            service: r.service,
            message: r.message,
        })
        .collect())
}

pub fn gen_logs(n: usize, seed: u64, start: u64) -> Vec<Doc> {
    let mut r = StdRng::seed_from_u64(seed);
    let services = ["api", "auth", "billing", "worker", "gateway"];
    (0..n)
        .map(|i| {
            let ip = format!(
                "10.{}.{}.{}",
                r.gen_range(0..255),
                r.gen_range(0..255),
                r.gen_range(1..255)
            );
            let uuid = format!(
                "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                r.gen::<u32>(),
                r.gen::<u16>(),
                r.gen::<u16>(),
                r.gen::<u16>(),
                r.gen::<u64>() & 0xffff_ffff_ffff
            );
            let roll = r.gen_range(0..100);
            let (level, message) = if roll < 70 {
                let m = match r.gen_range(0..3) {
                    0 => format!(
                        "request {} completed in {}ms",
                        r.gen_range(1000..99999),
                        r.gen_range(1..900)
                    ),
                    1 => format!("user {} logged in from {ip}", r.gen_range(1..5000)),
                    _ => format!("cache hit for key {uuid}"),
                };
                ("info", m)
            } else if roll < 90 {
                let m = if r.gen_range(0..2) == 0 {
                    format!("slow query took {}ms", r.gen_range(500..5000))
                } else {
                    format!("retrying connection to {ip} attempt {}", r.gen_range(1..6))
                };
                ("warn", m)
            } else {
                let m = match r.gen_range(0..3) {
                    0 => format!("upstream timeout after {}ms", r.gen_range(1000..30000)),
                    1 => format!("payment declined for order {}", r.gen_range(10000..99999)),
                    _ => format!("failed to connect to {ip}"),
                };
                ("error", m)
            };
            Doc {
                ts: start + i as u64 * 7,
                level: level.to_string(),
                service: services[r.gen_range(0..services.len())].to_string(),
                message,
            }
        })
        .collect()
}