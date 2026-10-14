use crate::Doc;

pub fn put(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(b);
            return;
        }
        buf.push(b | 0x80);
    }
}

pub fn get(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        if shift >= 64 {
            return None;
        }
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

pub fn take<'a>(buf: &'a [u8], pos: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = pos.checked_add(len)?;
    let s = buf.get(*pos..end)?;
    *pos = end;
    Some(s)
}

pub fn put_str(buf: &mut Vec<u8>, s: &str) {
    put(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

pub fn get_str(buf: &[u8], pos: &mut usize) -> Option<String> {
    let n = usize::try_from(get(buf, pos)?).ok()?;
    String::from_utf8(take(buf, pos, n)?.to_vec()).ok()
}

pub fn put_doc(buf: &mut Vec<u8>, d: &Doc) {
    put(buf, d.ts);
    put_str(buf, &d.level);
    put_str(buf, &d.service);
    put_str(buf, &d.message);
}

pub fn get_doc(buf: &[u8], pos: &mut usize) -> Option<Doc> {
    Some(Doc {
        ts: get(buf, pos)?,
        level: get_str(buf, pos)?,
        service: get_str(buf, pos)?,
        message: get_str(buf, pos)?,
    })
}

pub fn encode_deltas(ids: &[u32]) -> Vec<u8> {
    let mut b = Vec::new();
    put(&mut b, ids.len() as u64);
    let mut prev = 0u32;
    for &id in ids {
        put(&mut b, u64::from(id - prev));
        prev = id;
    }
    b
}

pub fn decode_deltas(buf: &[u8]) -> Option<Vec<u32>> {
    let mut pos = 0;
    let n = usize::try_from(get(buf, &mut pos)?).ok()?;
    let mut out = Vec::with_capacity(n.min(buf.len()));
    let mut prev = 0u32;
    for _ in 0..n {
        let d = u32::try_from(get(buf, &mut pos)?).ok()?;
        prev = prev.checked_add(d)?;
        out.push(prev);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, Rng, SeedableRng};
    use std::collections::BTreeSet;

    #[test]
    fn varint_roundtrip() {
        for v in [0u64, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            let mut b = Vec::new();
            put(&mut b, v);
            let mut p = 0;
            assert_eq!(get(&b, &mut p), Some(v));
            assert_eq!(p, b.len());
        }
    }

    #[test]
    fn delta_roundtrip() {
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..100 {
            let n = rng.gen_range(0..300);
            let set: BTreeSet<u32> = (0..n).map(|_| rng.gen_range(0..100_000)).collect();
            let ids: Vec<u32> = set.into_iter().collect();
            assert_eq!(decode_deltas(&encode_deltas(&ids)), Some(ids));
        }
    }

    #[test]
    fn truncated_input_is_rejected() {
        let enc = encode_deltas(&[5, 900, 70_000]);
        assert_eq!(decode_deltas(&enc[..enc.len() - 1]), None);
    }
}
