use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use hyperion_crypto::SealingKey;

use crate::types::{StorageError, WalRecord};

/// Append-only, fsync'd WAL — docs/28-storage-engine.md §Architecture:
/// "single source of truth for commit order." Backed by newline-delimited
/// JSON rather than a binary format: this crate is about proving the
/// atomicity and crash-recovery model correct, not wire-format performance
/// (28's own §Performance Analysis attributes write latency to the fsync,
/// not to encoding), and a text format is trivially inspectable while
/// debugging a from-scratch storage engine.
///
/// Real, optional encryption at rest (docs/28's own named "no encryption at rest" gap,
/// docs/16-privacy-architecture.md's Phase 8 CryptoShred prerequisite): [`Self::
/// open_for_append_encrypted`]/[`Self::replay_encrypted`] seal each individual record under its
/// own fresh nonce via [`hyperion_crypto::SealingKey`] (never one whole-file reseal per append,
/// which would make every write pay for the entire log's size) and hex-encode the sealed bytes
/// so the file stays the same real newline-delimited-text shape as the plaintext path -- a
/// torn/undecryptable trailing line still stops replay exactly like a torn plaintext JSON line
/// always has, never silently skipped mid-file.
pub struct Wal {
    file: File,
    seal: Option<SealingKey>,
}

impl Wal {
    /// Opens `path` for appending, creating it if it doesn't exist. Does
    /// not replay prior content — see [`Self::replay`].
    pub fn open_for_append(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Wal { file, seal: None })
    }

    /// As [`Self::open_for_append`], but every subsequent [`Self::append_and_fsync`] seals its
    /// record under `key` first -- see this struct's own doc comment.
    pub fn open_for_append_encrypted(path: &Path, key: [u8; 32]) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Wal {
            file,
            seal: Some(SealingKey::from_bytes(key)),
        })
    }

    /// Appends `record` and fsyncs before returning — docs/28 §Algorithms'
    /// "Write path" step 2: "the fsync'd append is the commit point."
    pub fn append_and_fsync(&mut self, record: &WalRecord) -> Result<(), StorageError> {
        let plaintext = serde_json::to_vec(record)?;
        let mut line = match &self.seal {
            Some(seal) => to_hex(&seal.seal(&plaintext)).into_bytes(),
            None => plaintext,
        };
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Reads every complete, well-formed record from `path` in commit
    /// order. Stops at the first record that fails to parse rather than
    /// erroring the whole replay — docs/28 §Failure Modes: "Process crash
    /// between WAL append and apply phase... detected and repaired by
    /// replay on restart," generalized here to a crash *during* the append
    /// itself, which can only ever leave a torn *trailing* line (appends
    /// are sequential and each prior line was already fsync'd), never
    /// corrupt an earlier record.
    pub fn replay(path: &Path) -> io::Result<Vec<WalRecord>> {
        Self::replay_lines(path, None)
    }

    /// As [`Self::replay`], but every line is expected to be a hex-encoded, `key`-sealed record
    /// (as [`Self::open_for_append_encrypted`] writes) -- a line that fails to decode as hex,
    /// fails real AEAD authentication under `key`, or doesn't parse as JSON after decryption is
    /// treated exactly like a torn plaintext line: replay stops there, never panics or skips it.
    pub fn replay_encrypted(path: &Path, key: [u8; 32]) -> io::Result<Vec<WalRecord>> {
        Self::replay_lines(path, Some(&SealingKey::from_bytes(key)))
    }

    fn replay_lines(path: &Path, seal: Option<&SealingKey>) -> io::Result<Vec<WalRecord>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let reader = BufReader::new(File::open(path)?);
        let mut records = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }
            match Self::parse_line(&line, seal) {
                Some(record) => records.push(record),
                None => break, // torn trailing write from an unclean shutdown
            }
        }
        Ok(records)
    }

    fn parse_line(line: &str, seal: Option<&SealingKey>) -> Option<WalRecord> {
        match seal {
            Some(seal) => {
                let sealed = from_hex(line)?;
                let plaintext = seal.open(&sealed).ok()?;
                serde_json::from_slice(&plaintext).ok()
            }
            None => serde_json::from_str(line).ok(),
        }
    }

    /// This crate's own named "garbage collection / compaction" gap (see the crate doc comment),
    /// closed for the version-retention slice: rewrites the WAL at `path` to contain only
    /// `records`, rather than the full history every object has ever gone through. Atomic via a
    /// same-filesystem `rename` over `path` — a crash mid-rewrite leaves either the untouched
    /// original WAL or the fully-written replacement, never a torn hybrid, the same "commit point"
    /// property [`Self::append_and_fsync`] already gives a single record. Returns a `Wal` already
    /// open for further appends against the rewritten file.
    pub fn compact(path: &Path, records: &[WalRecord]) -> Result<Self, StorageError> {
        Self::compact_impl(path, records, None)
    }

    /// As [`Self::compact`], but rewrites every record hex-encoded and sealed under `key`, and
    /// returns a `Wal` already open via [`Self::open_for_append_encrypted`].
    pub fn compact_encrypted(
        path: &Path,
        records: &[WalRecord],
        key: [u8; 32],
    ) -> Result<Self, StorageError> {
        Self::compact_impl(path, records, Some(key))
    }

    fn compact_impl(
        path: &Path,
        records: &[WalRecord],
        key: Option<[u8; 32]>,
    ) -> Result<Self, StorageError> {
        let seal = key.map(SealingKey::from_bytes);

        let mut tmp_path = path.as_os_str().to_os_string();
        tmp_path.push(".compact.tmp");
        let tmp_path = PathBuf::from(tmp_path);

        {
            let mut tmp = File::create(&tmp_path)?;
            for record in records {
                let plaintext = serde_json::to_vec(record)?;
                let mut line = match &seal {
                    Some(seal) => to_hex(&seal.seal(&plaintext)).into_bytes(),
                    None => plaintext,
                };
                line.push(b'\n');
                tmp.write_all(&line)?;
            }
            tmp.sync_all()?;
        }
        std::fs::rename(&tmp_path, path)?;

        Ok(match key {
            Some(key) => Self::open_for_append_encrypted(path, key)?,
            None => Self::open_for_append(path)?,
        })
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").expect("writing to a String never fails");
    }
    out
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_exactly() {
        let bytes = vec![0u8, 1, 255, 16, 128, 7];
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn odd_length_hex_fails_to_decode() {
        assert_eq!(from_hex("abc"), None);
    }

    #[test]
    fn non_hex_characters_fail_to_decode() {
        assert_eq!(from_hex("zz"), None);
    }
}
