use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Metrics {
    pub ingested: AtomicU64,
    pub searches: AtomicU64,
    pub skipped_bloom: AtomicU64,
    pub skipped_time: AtomicU64,
}

pub fn add(counter: &AtomicU64, n: u64) {
    counter.fetch_add(n, Ordering::Relaxed);
}

impl Metrics {
    pub fn render(&self) -> String {
        let g = |c: &AtomicU64| c.load(Ordering::Relaxed);
        format!(
            "# TYPE loglens_ingested_lines_total counter\n\
             loglens_ingested_lines_total {}\n\
             # TYPE loglens_searches_total counter\n\
             loglens_searches_total {}\n\
             # TYPE loglens_segments_skipped_total counter\n\
             loglens_segments_skipped_total{{reason=\"bloom\"}} {}\n\
             loglens_segments_skipped_total{{reason=\"time\"}} {}\n",
            g(&self.ingested),
            g(&self.searches),
            g(&self.skipped_bloom),
            g(&self.skipped_time)
        )
    }
}
