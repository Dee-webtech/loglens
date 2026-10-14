use std::cmp::Ordering;

pub fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

pub fn difference(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() {
        if j >= b.len() {
            out.push(a[i]);
            i += 1;
            continue;
        }
        match a[i].cmp(&b[j]) {
            Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, Rng, SeedableRng};
    use std::collections::BTreeSet;

    fn rand_set(rng: &mut StdRng) -> Vec<u32> {
        let n = rng.gen_range(0..200);
        let s: BTreeSet<u32> = (0..n).map(|_| rng.gen_range(0..500)).collect();
        s.into_iter().collect()
    }

    #[test]
    fn matches_brute_force() {
        let mut rng = StdRng::seed_from_u64(7);
        for _ in 0..300 {
            let a = rand_set(&mut rng);
            let b = rand_set(&mut rng);
            let want_i: Vec<u32> = a.iter().copied().filter(|x| b.contains(x)).collect();
            let want_d: Vec<u32> = a.iter().copied().filter(|x| !b.contains(x)).collect();
            assert_eq!(intersect(&a, &b), want_i);
            assert_eq!(difference(&a, &b), want_d);
        }
    }
}