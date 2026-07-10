use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use crate::types::{StorageError, WalRecord};

/// Append-only, fsync'd WAL — docs/28-storage-engine.md §Architecture:
/// "single source of truth for commit order." Backed by newline-delimited
/// JSON rather than a binary format: this crate is about proving the
/// atomicity and crash-recovery model correct, not wire-format performance
/// (28's own §Performance Analysis attributes write latency to the fsync,
/// not to encoding), and a text format is trivially inspectable while
/// debugging a from-scratch storage engine.
pub struct Wal {
    file: File,
}

impl Wal {
    /// Opens `path` for appending, creating it if it doesn't exist. Does
    /// not replay prior content — see [`Self::replay`].
    pub fn open_for_append(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Wal { file })
    }

    /// Appends `record` and fsyncs before returning — docs/28 §Algorithms'
    /// "Write path" step 2: "the fsync'd append is the commit point."
    pub fn append_and_fsync(&mut self, record: &WalRecord) -> Result<(), StorageError> {
        let mut line = serde_json::to_vec(record)?;
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
            match serde_json::from_str::<WalRecord>(&line) {
                Ok(record) => records.push(record),
                Err(_) => break, // torn trailing write from an unclean shutdown
            }
        }
        Ok(records)
    }
}
