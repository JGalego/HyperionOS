use std::collections::HashMap;
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
}

impl StorageEngine {
    /// Opens (or creates) the WAL at `path` and replays it to rebuild both
    /// materialized views from scratch — docs/28 §Recovery Mechanisms: "On
    /// restart, the engine replays the WAL... a standard redo-log
    /// recovery."
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let records = Wal::replay(&path)?;

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

        let wal = Wal::open_for_append(&path)?;

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
}
