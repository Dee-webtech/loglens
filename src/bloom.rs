pub struct Bloom {
    bits: Vec<u8>,
    k: u32,
}

fn fnv(s: &str, seed: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl Bloom {
    pub fn with_capacity(n: usize) -> Bloom {
        let bytes = (n.max(1) * 10).div_ceil(8);
        Bloom {
            bits: vec![0; bytes.max(8)],
            k: 7,
        }
    }

    fn positions(&self, s: &str) -> impl Iterator<Item = usize> {
        let m = (self.bits.len() * 8) as u64;
        let h1 = fnv(s, 0);
        let h2 = fnv(s, 0x9e37_79b9_7f4a_7c15) | 1;
        (0..u64::from(self.k)).map(move |i| (h1.wrapping_add(i.wrapping_mul(h2)) % m) as usize)
    }

    pub fn insert(&mut self, s: &str) {
        let pos: Vec<usize> = self.positions(s).collect();
        for p in pos {
            self.bits[p / 8] |= 1 << (p % 8);
        }
    }

    pub fn contains(&self, s: &str) -> bool {
        self.positions(s)
            .all(|p| (self.bits[p / 8] & (1 << (p % 8))) != 0)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = vec![self.k as u8];
        v.extend_from_slice(&self.bits);
        v
    }

    pub fn from_bytes(b: &[u8]) -> Option<Bloom> {
        let (k, bits) = b.split_first()?;
        if bits.is_empty() || *k == 0 {
            return None;
        }
        Some(Bloom {
            bits: bits.to_vec(),
            k: u32::from(*k),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, Rng, SeedableRng};

    #[test]
    fn no_false_negatives() {
        let mut rng = StdRng::seed_from_u64(11);
        let items: Vec<String> = (0..5000)
            .map(|_| format!("t{}x{}", rng.gen_range(0..u32::MAX), rng.gen_range(0..1000)))
            .collect();
        let mut bloom = Bloom::with_capacity(items.len());
        for s in &items {
            bloom.insert(s);
        }
        assert!(items.iter().all(|s| bloom.contains(s)));
        let restored = Bloom::from_bytes(&bloom.to_bytes()).unwrap();
        assert!(items.iter().all(|s| restored.contains(s)));
    }
}