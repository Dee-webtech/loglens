use criterion::{criterion_group, criterion_main, Criterion};
use loglens::ingest::{gen_logs, Tenant};
use loglens::metrics::Metrics;
use loglens::query;
use loglens::search::search;

const LINES: usize = 100_000;
const SEAL: usize = 1 << 20;

fn ingest(c: &mut Criterion) {
    let docs = gen_logs(LINES, 1, 1_000_000);
    let mut g = c.benchmark_group("ingest");
    g.sample_size(10);
    g.bench_function("100k_lines", |b| {
        b.iter(|| {
            let dir = tempfile::tempdir().unwrap();
            let t = Tenant::open(dir.path()).unwrap();
            for chunk in docs.chunks(1000) {
                t.append(chunk.to_vec(), SEAL).unwrap();
            }
        })
    });
    g.finish();
}

fn search_bench(c: &mut Criterion) {
    let docs = gen_logs(LINES, 1, 1_000_000);
    let dir = tempfile::tempdir().unwrap();
    let t = Tenant::open(dir.path()).unwrap();
    for chunk in docs.chunks(1000) {
        t.append(chunk.to_vec(), SEAL).unwrap();
    }
    let m = Metrics::default();

    let rare = query::parse(&docs[50_000].message);
    let mut g = c.benchmark_group("rare_term");
    for (name, prune) in [("prune", true), ("no_prune", false)] {
        g.bench_function(name, |b| {
            b.iter(|| search(&t, &rare, 0, u64::MAX, 100, &m, prune))
        });
    }
    g.finish();

    let errors = query::parse("level:error");
    let (from, to) = (docs[0].ts, docs[2000].ts);
    let mut g = c.benchmark_group("time_window");
    for (name, prune) in [("prune", true), ("no_prune", false)] {
        g.bench_function(name, |b| {
            b.iter(|| search(&t, &errors, from, to, 100, &m, prune))
        });
    }
    g.finish();
}

criterion_group!(benches, ingest, search_bench);
criterion_main!(benches);
