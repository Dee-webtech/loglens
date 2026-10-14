use crate::{varint, Doc, Error};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub struct Wal {
    file: File,
}

fn sum4(p: &[u8]) -> [u8; 4] {
    let d = Sha256::digest(p);
    [d[0], d[1], d[2], d[3]]
}

fn read_record(raw: &[u8], pos: usize) -> Option<(Vec<Doc>, usize)> {
    let head = raw.get(pos..pos.checked_add(8)?)?;
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    let start = pos + 8;
    let payload = raw.get(start..start.checked_add(len)?)?;
    if sum4(payload).as_slice() != &head[4..8] {
        return None;
    }
    let mut p = 0;
    let n = usize::try_from(varint::get(payload, &mut p)?).ok()?;
    let mut docs = Vec::new();
    for _ in 0..n {
        docs.push(varint::get_doc(payload, &mut p)?);
    }
    Some((docs, start + len))
}

impl Wal {
    pub fn open(path: &Path) -> Result<(Wal, Vec<Doc>), Error> {
        let raw = if path.exists() {
            fs::read(path)?
        } else {
            Vec::new()
        };
        let mut docs = Vec::new();
        let mut pos = 0usize;
        while let Some((batch, next)) = read_record(&raw, pos) {
            docs.extend(batch);
            pos = next;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(pos as u64)?;
        file.seek(SeekFrom::Start(pos as u64))?;
        Ok((Wal { file }, docs))
    }

    pub fn append(&mut self, docs: &[Doc]) -> Result<(), Error> {
        let mut payload = Vec::new();
        varint::put(&mut payload, docs.len() as u64);
        for d in docs {
            varint::put_doc(&mut payload, d);
        }
        let mut rec = Vec::with_capacity(payload.len() + 8);
        rec.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        rec.extend_from_slice(&sum4(&payload));
        rec.extend_from_slice(&payload);
        self.file.write_all(&rec)?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn reset(&mut self) -> Result<(), Error> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.sync_data()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::gen_logs;

    #[test]
    fn replays_complete_records_and_drops_torn_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wal.log");
        let a = gen_logs(10, 1, 100);
        let b = gen_logs(5, 2, 200);
        {
            let (mut wal, replayed) = Wal::open(&path).unwrap();
            assert!(replayed.is_empty());
            wal.append(&a).unwrap();
            wal.append(&b).unwrap();
        }
        let mut raw = fs::read(&path).unwrap();
        raw.extend_from_slice(&[200, 0, 0, 0, 1, 2, 3, 4, 9, 9]);
        fs::write(&path, raw).unwrap();

        let (mut wal, replayed) = Wal::open(&path).unwrap();
        let mut want = a.clone();
        want.extend(b.clone());
        assert_eq!(replayed, want);

        let c = gen_logs(3, 3, 300);
        wal.append(&c).unwrap();
        drop(wal);
        let (_, again) = Wal::open(&path).unwrap();
        want.extend(c);
        assert_eq!(again, want);
    }
}
