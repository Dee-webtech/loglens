use crate::Doc;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

fn is_uuid(c: &[char]) -> bool {
    if c.len() < 36 {
        return false;
    }
    c[..36].iter().enumerate().all(|(k, ch)| {
        if matches!(k, 8 | 13 | 18 | 23) {
            *ch == '-'
        } else {
            ch.is_ascii_hexdigit()
        }
    })
}

pub fn normalize(msg: &str) -> String {
    let c: Vec<char> = msg.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if is_uuid(&c[i..]) {
            out.push_str("<uuid>");
            i += 36;
            continue;
        }
        if c[i].is_ascii_digit() {
            let mut j = i;
            while j < c.len() && (c[j].is_ascii_digit() || c[j] == '.') {
                j += 1;
            }
            while c[j - 1] == '.' {
                j -= 1;
            }
            let run: String = c[i..j].iter().collect();
            let parts: Vec<&str> = run.split('.').collect();
            let is_ip = parts.len() == 4 && parts.iter().all(|p| !p.is_empty());
            out.push_str(if is_ip { "<ip>" } else { "<n>" });
            i = j;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

pub fn count_templates<'a>(
    docs: impl Iterator<Item = &'a Doc>,
    level: Option<&str>,
) -> HashMap<String, u64> {
    let want = level.map(str::to_lowercase);
    let mut counts = HashMap::new();
    for d in docs {
        if let Some(l) = &want {
            if d.level.to_lowercase() != *l {
                continue;
            }
        }
        *counts.entry(normalize(&d.message)).or_insert(0) += 1;
    }
    counts
}

pub fn top_k(counts: HashMap<String, u64>, k: usize) -> Vec<(String, u64)> {
    let mut heap = BinaryHeap::new();
    for (t, c) in counts {
        heap.push(Reverse((c, Reverse(t))));
        if heap.len() > k {
            heap.pop();
        }
    }
    heap.into_sorted_vec()
        .into_iter()
        .map(|Reverse((c, Reverse(t)))| (t, c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, Rng, SeedableRng};

    #[test]
    fn normalizes_numbers_ips_and_uuids() {
        let m = "user 42 from 10.0.0.1 id 550e8400-e29b-41d4-a716-446655440000 took 7ms.";
        assert_eq!(normalize(m), "user <n> from <ip> id <uuid> took <n>ms.");
    }

    #[test]
    fn heap_matches_full_sort() {
        let mut rng = StdRng::seed_from_u64(21);
        for k in [1usize, 5, 17, 500] {
            let counts: HashMap<String, u64> = (0..200)
                .map(|i| (format!("template {i}"), rng.gen_range(1..20)))
                .collect();
            let mut full: Vec<(String, u64)> = counts.clone().into_iter().collect();
            full.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            full.truncate(k);
            assert_eq!(top_k(counts, k), full);
        }
    }
}