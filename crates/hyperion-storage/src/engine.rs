use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};

use crate::types::{ObjectId, StorageError, VersionId, VersionRecord, WalRecord};
use crate::wal::Wal;

struct Inner {
    wal: Wal,
    metadata: HashMap<ObjectId, serde_json::Value>,
    version_pointer: HashMap<ObjectId, VersionId>,
    version_chain: HashMap<VersionId, VersionRecord>,
    next_object_id: u64,
    next_version_id: u64,
}

/// docs/28-storage-engine.md's Storage Engine, narrowed to the two
/// materialized views this crate currently implements (metadata + version
/// chain — see [`crate::types::WalRecord`]'s docs for what's deferred: the
/// content-addressed blob store, the graph index, and the vector index).
///
/// The load-bearing architectural decision docs/28 §Architecture states —
/// "the four stores are materialized views of the WAL, not four
/// independently-committed systems" — holds here for the two views that do
/// exist: [`Self::open`] rebuilds both entirely by replaying the WAL, never
/// from any separately-persisted state.
///
/// One further simplification worth naming: the real design applies a
/// committed record to its materialized views *asynchronously*, behind a
/// `pending_overlay` that guarantees read-your-writes before the apply
/// phase catches up (docs/28 §Algorithms' "Write path" step 3). This engine
/// applies synchronously, under the same lock as the WAL append — trivially
/// giving read-your-writes without needing an overlay, at the cost of the
/// throughput a pipelined async apply would buy. Revisit if/when a real
/// workload makes synchronous apply the bottleneck.
pub struct StorageEngine {
    inner: Mutex<Inner>,
    path: PathBuf,
    encryption_key: Option<[u8; 32]>,
}

impl StorageEngine {
    /// Opens (or creates) the WAL at `path` and replays it to rebuild both
    /// materialized views from scratch — docs/28 §Recovery Mechanisms: "On
    /// restart, the engine replays the WAL... a standard redo-log
    /// recovery."
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        Self::open_impl(path, None)
    }

    /// As [`Self::open`], but the WAL is encrypted at rest under `key` (see [`crate::wal::Wal`]'s
    /// own doc comment) -- both the initial replay and every subsequent append/compact use the
    /// same real per-record sealing.
    pub fn open_encrypted(path: impl AsRef<Path>, key: [u8; 32]) -> Result<Self, StorageError> {
        Self::open_impl(path, Some(key))
    }

    fn open_impl(path: impl AsRef<Path>, key: Option<[u8; 32]>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let records = match key {
            Some(key) => Wal::replay_encrypted(&path, key)?,
            None => Wal::replay(&path)?,
        };

        let mut metadata = HashMap::new();
        let mut version_pointer = HashMap::new();
        let mut version_chain = HashMap::new();
        let mut next_object_id = 0u64;
        let mut next_version_id = 0u64;

        for record in records {
            metadata.insert(record.object_id, record.metadata.clone());
            version_pointer.insert(record.object_id, record.new_version);
            version_chain.insert(
                record.new_version,
                VersionRecord {
                    object_id: record.object_id,
                    parent_version: record.prev_version,
                    metadata: record.metadata,
                },
            );
            next_object_id = next_object_id.max(record.object_id.0 + 1);
            next_version_id = next_version_id.max(record.new_version.0 + 1);
        }

        let wal = match key {
            Some(key) => Wal::open_for_append_encrypted(&path, key)?,
            None => Wal::open_for_append(&path)?,
        };

        Ok(StorageEngine {
            inner: Mutex::new(Inner {
                wal,
                metadata,
                version_pointer,
                version_chain,
                next_object_id,
                next_version_id,
            }),
            path,
            encryption_key: key,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `put_object` — docs/28 §Interfaces / APIs, folding `begin_txn` +
    /// `put_object` + `commit_txn` into one call since this engine's apply
    /// is synchronous (see the struct docs) and has nothing a multi-call
    /// transaction handle would buy yet.
    ///
    /// `object_id: None` creates a new object. `expected_version` is the
    /// compare-and-swap check from docs/28 §Algorithms step 2: pass `None`
    /// for a brand-new object, or the version the caller last observed for
    /// an existing one; a mismatch — including another writer having
    /// advanced it concurrently — returns
    /// [`StorageError::ConcurrentWriteConflict`] rather than silently
    /// overwriting.
    pub fn put_object(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        object_id: Option<ObjectId>,
        expected_version: Option<VersionId>,
        metadata: serde_json::Value,
    ) -> Result<(ObjectId, VersionId), StorageError> {
        monitor
            .check_rights_ok_result(token, RightsMask::WRITE)
            .map_err(|_| StorageError::Unauthorized)?;

        let mut inner = self.inner.lock().unwrap();

        let object_id = object_id.unwrap_or_else(|| {
            let id = ObjectId(inner.next_object_id);
            inner.next_object_id += 1;
            id
        });

        let current = inner.version_pointer.get(&object_id).copied();
        if current != expected_version {
            return Err(StorageError::ConcurrentWriteConflict {
                object_id,
                expected: expected_version,
                found: current,
            });
        }

        let new_version = VersionId(inner.next_version_id);

        let record = WalRecord {
            object_id,
            prev_version: current,
            new_version,
            metadata: metadata.clone(),
            actor_origin: token.origin().0,
        };

        // Atomicity boundary: the fsync'd append is the commit point
        // (docs/28 §Algorithms step 2). Nothing below this line can fail
        // the transaction — it can only fail to have *happened* yet.
        inner.wal.append_and_fsync(&record)?;
        inner.next_version_id += 1;

        inner.metadata.insert(object_id, metadata.clone());
        inner.version_pointer.insert(object_id, new_version);
        inner.version_chain.insert(
            new_version,
            VersionRecord {
                object_id,
                parent_version: current,
                metadata,
            },
        );

        Ok((object_id, new_version))
    }

    /// `get_object` — docs/28 §Interfaces / APIs. `version: None` reads the
    /// current head; `Some(v)` reads that specific point in the object's
    /// version chain.
    pub fn get_object(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        object_id: ObjectId,
        version: Option<VersionId>,
    ) -> Result<serde_json::Value, StorageError> {
        monitor
            .check_rights_ok_result(token, RightsMask::READ)
            .map_err(|_| StorageError::Unauthorized)?;

        let inner = self.inner.lock().unwrap();
        match version {
            Some(v) => inner
                .version_chain
                .get(&v)
                .filter(|rec| rec.object_id == object_id)
                .map(|rec| rec.metadata.clone())
                .ok_or(StorageError::NotFound),
            None => inner
                .metadata
                .get(&object_id)
                .cloned()
                .ok_or(StorageError::NotFound),
        }
    }

    /// The object's current version pointer, if it exists — used by
    /// callers that need to know what `expected_version` to pass to a
    /// subsequent `put_object` without doing a full `get_object` read.
    pub fn current_version(&self, object_id: ObjectId) -> Option<VersionId> {
        self.inner
            .lock()
            .unwrap()
            .version_pointer
            .get(&object_id)
            .copied()
    }

    /// This crate's own named "garbage collection / compaction" gap (see the crate doc comment:
    /// "nothing here is ever deleted or compacted yet"), closed for the one slice entirely
    /// internal to this engine's own two materialized views: version retention. docs/28's fuller
    /// design tiers retention across N versions/T days into periodic snapshots; this crate has no
    /// timestamp on a [`VersionRecord`]/[`WalRecord`] to key a time-based tier by, so — matching
    /// this session's own established simplification ("one real, general mechanism, not retention
    /// *classes*") — every object's history collapses unconditionally to its current head, which
    /// becomes its own new genesis (`parent_version: None`). The content-addressed blob store,
    /// graph/vector index rebuild, and cross-device sync compaction docs/28 also names remain
    /// genuinely out of scope — none of those subsystems exist in this crate.
    ///
    /// Rewrites the on-disk WAL too (via [`Wal::compact`]), not just the in-memory
    /// `version_chain` — a real restart must not resurrect history this call already dropped.
    /// The WAL rewrite happens *before* the in-memory prune, mirroring [`Self::put_object`]'s own
    /// "durable append is the commit point, nothing after it can fail the transaction" ordering:
    /// if the rewrite fails, the in-memory state is left exactly as it was, still consistent with
    /// the (untouched) on-disk WAL.
    ///
    /// Returns the number of historical [`VersionRecord`]s evicted, so a caller can log or audit
    /// exactly what a sweep did.
    pub fn compact(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
    ) -> Result<usize, StorageError> {
        monitor
            .check_rights_ok_result(token, RightsMask::WRITE)
            .map_err(|_| StorageError::Unauthorized)?;

        let mut inner = self.inner.lock().unwrap();

        let new_records: Vec<WalRecord> = inner
            .version_pointer
            .iter()
            .map(|(&object_id, &head_version)| WalRecord {
                object_id,
                prev_version: None,
                new_version: head_version,
                metadata: inner
                    .metadata
                    .get(&object_id)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                actor_origin: token.origin().0,
            })
            .collect();

        inner.wal = match self.encryption_key {
            Some(key) => Wal::compact_encrypted(&self.path, &new_records, key)?,
            None => Wal::compact(&self.path, &new_records)?,
        };

        let before = inner.version_chain.len();
        let heads: HashSet<VersionId> = inner.version_pointer.values().copied().collect();
        inner.version_chain.retain(|id, _| heads.contains(id));
        for record in inner.version_chain.values_mut() {
            record.parent_version = None;
        }

        Ok(before - inner.version_chain.len())
    }
}
