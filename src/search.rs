use crate::ingest::{rlock, Tenant};
use crate::metrics::{add, Metrics};
use crate::postings::{difference, intersect};
use crate::query::Query;
use crate::segment::Segment;
use crate::top::{count_templates, top_k};
use crate::Doc;

fn match_ids(q: &Query, n: usize, post: impl Fn(&str) -> Option<Vec<u32>>) -> Vec<u32> {
    let mut ids: Vec<u32> = if q.must.is_empty() {
        (0..n as u32).collect()
    } else {
        let mut lists = Vec::new();
        for t in &q.must {
            match post(t) {
                Some(p) => lists.push(p),
                None => return Vec::new(),
            }
        }
        lists.sort_by_key(|l| l.len());
        let mut it = lists.into_iter();
        let mut acc = it.next().unwrap_or_default();
        for l in it {
            if acc.is_empty() {
                break;
            }
            acc = intersect(&acc, &l);
        }
        acc
    };
    for t in &q.not {
        if ids.is_empty() {
            break;
        }
        if let Some(p) = post(t) {
            ids = difference(&ids, &p);
        }
    }
    ids
}

fn from_segment(
    seg: &Segment,
    q: &Query,
    from: u64,
    to: u64,
    limit: usize,
    m: &Metrics,
    prune: bool,
) -> Vec<Doc> {
    if prune {
        if seg.max_ts < from || seg.min_ts > to {
            add(&m.skipped_time, 1);
            return Vec::new();
        }
        if q.must.iter().any(|t| !seg.may_contain(t)) {
            add(&m.skipped_bloom, 1);
            return Vec::new();
        }
    }
    let ids = match_ids(q, seg.doc_count(), |t| seg.postings(t));
    ids.iter()
        .rev()
        .map(|&i| seg.doc(i))
        .filter(|d| d.ts >= from && d.ts <= to)
        .take(limit)
        .cloned()
        .collect()
}

pub fn search(
    t: &Tenant,
    q: &Query,
    from: u64,
    to: u64,
    limit: usize,
    m: &Metrics,
    prune: bool,
) -> Vec<Doc> {
    add(&m.searches, 1);
    let st = rlock(&t.state);
    let ids = match_ids(q, st.mem.len(), |x| st.mem.postings(x).map(|p| p.to_vec()));
    let mut out: Vec<Doc> = ids
        .iter()
        .map(|&i| st.mem.doc(i))
        .filter(|d| d.ts >= from && d.ts <= to)
        .cloned()
        .collect();
    for seg in &st.segs {
        out.extend(from_segment(seg, q, from, to, limit, m, prune));
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.ts));
    out.truncate(limit);
    out
}

pub fn top_templates(t: &Tenant, k: usize, level: Option<&str>) -> Vec<(String, u64)> {
    let st = rlock(&t.state);
    let docs = st
        .mem
        .docs()
        .iter()
        .chain(st.segs.iter().flat_map(|s| s.docs().iter()));
    top_k(count_templates(docs, level), k)
}
