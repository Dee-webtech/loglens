use crate::bloom::Bloom;
use crate::query::doc_terms;
use crate::{varint, Doc, Error};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const MAGIC: &[u8; 4] = b"LLS1";
const HEADER: usize = 20;

pub struct Segment {
    pub id: u64,
    pub min_ts: u64,
    pub max_ts: u64,
    docs: Vec<Doc>,
    terms: Vec<(String, Vec<u8>)>,
    bloom: Bloom,
}

fn bad(id: u64) -> Error {
    Error::Corrupt(format!("segment {id}"))
}

fn read_len(b: &[u8], pos: &mut usize, id: u64) -> Result<usize, Error> {
    varint::get(b, pos)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| bad(id))
}

impl Segment {
    pub fn build(id: u64, mut docs: Vec<Doc>) -> Segment {
        docs.sort_by_key(|d| d.ts);
        let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for (i, d) in docs.iter().enumerate() {
            for t in doc_terms(d) {
                map.entry(t).or_default().push(i as u32);
            }
        }
        let mut bloom = Bloom::with_capacity(map.len());
        let terms: Vec<(String, Vec<u8>)> = map
            .into_iter()
            .map(|(t, ids)| {
                bloom.insert(&t);
                let enc = varint::encode_deltas(&ids);
                (t, enc)
            })
            .collect();
        let min_ts = docs.first().map(|d| d.ts).unwrap_or(0);
        let max_ts = docs.last().map(|d| d.ts).unwrap_or(0);
        Segment {
            id,
            min_ts,
            max_ts,
            docs,
            terms,
            bloom,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&self.min_ts.to_le_bytes());
        b.extend_from_slice(&self.max_ts.to_le_bytes());
        varint::put(&mut b, self.docs.len() as u64);
        let bl = self.bloom.to_bytes();
        varint::put(&mut b, bl.len() as u64);
        b.extend_from_slice(&bl);
        varint::put(&mut b, self.terms.len() as u64);
        for (t, p) in &self.terms {
            varint::put_str(&mut b, t);
            varint::put(&mut b, p.len() as u64);
            b.extend_from_slice(p);
        }
        for d in &self.docs {
            varint::put_doc(&mut b, d);
        }
        let sum = Sha256::digest(&b);
        b.extend_from_slice(&sum);
        b
    }

    pub fn write(&self, path: &Path) -> Result<(), Error> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, self.encode())?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(id: u64, path: &Path) -> Result<Segment, Error> {
        let raw = fs::read(path)?;
        Segment::decode(id, &raw)
    }

    pub fn decode(id: u64, raw: &[u8]) -> Result<Segment, Error> {
        if raw.len() < HEADER + 32 {
            return Err(bad(id));
        }
        let (body, sum) = raw.split_at(raw.len() - 32);
        if Sha256::digest(body).as_slice() != sum || body[..4] != MAGIC[..] {
            return Err(bad(id));
        }
        let min_ts = u64::from_le_bytes(body[4..12].try_into().map_err(|_| bad(id))?);
        let max_ts = u64::from_le_bytes(body[12..20].try_into().map_err(|_| bad(id))?);
        let mut pos = HEADER;
        let n_docs = read_len(body, &mut pos, id)?;
        let bl = read_len(body, &mut pos, id)?;
        let bloom = varint::take(body, &mut pos, bl)
            .and_then(Bloom::from_bytes)
            .ok_or_else(|| bad(id))?;
        let n_terms = read_len(body, &mut pos, id)?;
        let mut terms = Vec::with_capacity(n_terms.min(1 << 20));
        for _ in 0..n_terms {
            let t = varint::get_str(body, &mut pos).ok_or_else(|| bad(id))?;
            let pl = read_len(body, &mut pos, id)?;
            let p = varint::take(body, &mut pos, pl).ok_or_else(|| bad(id))?;
            terms.push((t, p.to_vec()));
        }
        let mut docs = Vec::with_capacity(n_docs.min(1 << 20));
        for _ in 0..n_docs {
            docs.push(varint::get_doc(body, &mut pos).ok_or_else(|| bad(id))?);
        }
        Ok(Segment {
            id,
            min_ts,
            max_ts,
            docs,
            terms,
            bloom,
        })
    }

    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }

    pub fn docs(&self) -> &[Doc] {
        &self.docs
    }

    pub fn doc(&self, id: u32) -> &Doc {
        &self.docs[id as usize]
    }

    pub fn terms(&self) -> impl Iterator<Item = &str> {
        self.terms.iter().map(|(t, _)| t.as_str())
    }

    pub fn may_contain(&self, term: &str) -> bool {
        self.bloom.contains(term)
    }

    pub fn postings(&self, term: &str) -> Option<Vec<u32>> {
        let i = self
            .terms
            .binary_search_by(|(t, _)| t.as_str().cmp(term))
            .ok()?;
        varint::decode_deltas(&self.terms[i].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::gen_logs;

    #[test]
    fn roundtrip_preserves_everything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seg-00000001.seg");
        let seg = Segment::build(1, gen_logs(500, 5, 1_000));
        seg.write(&path).unwrap();
        let back = Segment::load(1, &path).unwrap();
        assert_eq!(back.docs(), seg.docs());
        assert_eq!((back.min_ts, back.max_ts), (seg.min_ts, seg.max_ts));
        let names: Vec<&str> = seg.terms().collect();
        assert!(!names.is_empty());
        for t in names {
            assert_eq!(back.postings(t), seg.postings(t));
            assert!(back.may_contain(t));
        }
        assert!(back.postings("zzzmissing").is_none());
    }

    #[test]
    fn corruption_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seg-00000002.seg");
        Segment::build(2, gen_logs(200, 9, 1_000))
            .write(&path)
            .unwrap();
        let mut raw = std::fs::read(&path).unwrap();
        let mid = raw.len() / 2;
        raw[mid] ^= 0xff;
        std::fs::write(&path, raw).unwrap();
        assert!(matches!(Segment::load(2, &path), Err(Error::Corrupt(_))));
    }
}
