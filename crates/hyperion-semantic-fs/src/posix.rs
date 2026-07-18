//! The real POSIX/FUSE mount this crate's own doc comment previously named as deferred:
//! `fs.mount_posix` now exists, and every VFS call it serves resolves through
//! [`SemanticFilesystem`]'s own already-real, capability-gated methods (`all_paths`,
//! `get_object`, `write_back`, `mkcollection`, `resolve_path`) — this module adds no second
//! permission system and no parallel data model, only the translation FUSE itself requires
//! between kernel inode numbers and this crate's own synthesized paths.
//!
//! **Mount-time scope, honestly named.** A POSIX mount has exactly one caller identity for its
//! whole lifetime (the `CapabilityMonitor`/`CapabilityToken` pair it's mounted with) — the kernel
//! VFS interface carries a uid/gid per request, not a Hyperion capability token, so per-request
//! re-authorization the way every other entry point in this crate already does isn't reachable
//! here. This mirrors this crate's own already-named "coarse capability-rights check, same one
//! every call into `hyperion-knowledge-graph` already performs" scope, just fixed for the whole
//! mount rather than re-checked per native call.
//!
//! **Namespace shape.** There is no single predefined "root query" in this crate's model (see
//! `lib.rs`'s own "query-as-navigation" framing) — so the mount's root shows every path this
//! caller can currently see (`SemanticFilesystem::all_paths`), and every synthesized path
//! (`"<object_type>/<title>"` for an ordinary object, a caller-pinned name for a real Collection)
//! becomes a real directory tree by scanning that flat path list for shared prefixes. A path with
//! deeper entries beneath it (real Collection members, `write_back`'s own nested-path convention)
//! renders as a directory even when that same path is also a real object in its own right — a
//! deliberate, honest choice (a Collection's own object attributes are not surfaced via `getattr`
//! on its directory inode; only real leaf objects get real per-object timestamps/size).
//!
//! **Deferred, and why:** `unlink`/`rmdir` are not implemented (`ENOSYS`) — [`SemanticFilesystem`]
//! itself exposes no delete operation to translate to (docs/09's tombstone-based delete lives one
//! layer down, in `hyperion-knowledge-graph`, and is not yet threaded through this crate's own
//! public API) — adding real FUSE deletion would mean inventing a new capability this crate
//! doesn't have yet, not just adapting an existing one, so it stays out of this pass.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fuser::{
    BackgroundSession, Config, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation,
    INodeNo, LockOwner, MountOption, OpenFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request, WriteFlags,
};
use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_knowledge_graph::{NodeId, NodeRecord};

use crate::engine::SemanticFilesystem;
use crate::types::DirEntry;

const ROOT_INO: u64 = 1;
/// How long the kernel may cache an entry/attribute reply before re-asking — short, since a real
/// Knowledge Graph write from another caller (or another mount) should become visible promptly,
/// matching this crate's own "no unbounded staleness" convention elsewhere (`VirtualFolder`'s own
/// TTL, docs/10 §Performance Analysis).
const TTL: Duration = Duration::from_secs(1);

/// One node's real place in the mounted tree, resolved from the flat, synthesized path list
/// [`SemanticFilesystem::all_paths`] returns.
enum ChildKind {
    /// Not itself a leaf object at this exact path, or a real object that also has further
    /// entries nested beneath it (a Collection with members) — either way, a real directory.
    Directory,
    /// A leaf object with no further path segments beneath it.
    File(NodeId),
}

fn full_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// docs/10 §Algorithms' "Path synthesis," read back out as a one-level directory listing: every
/// immediate child name under `prefix` among the caller's full, flat, visible path list, with
/// [`ChildKind::Directory`] always winning over [`ChildKind::File`] for the same name regardless
/// of which entry happened to be scanned first — a Collection with real members must render as a
/// directory even though its own synthesized path is also, individually, a real leaf entry.
fn children_of(prefix: &str, entries: &[DirEntry]) -> BTreeMap<String, ChildKind> {
    let scan_prefix = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };
    let mut children: BTreeMap<String, ChildKind> = BTreeMap::new();
    for entry in entries {
        let Some(rest) = entry.path.strip_prefix(scan_prefix.as_str()) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        match rest.split_once('/') {
            Some((name, _)) => {
                children.insert(name.to_string(), ChildKind::Directory);
            }
            None => {
                children
                    .entry(rest.to_string())
                    .or_insert(ChildKind::File(entry.object_id));
            }
        }
    }
    children
}

fn dir_attr(ino: u64, uid: u32, gid: u32) -> FileAttr {
    let now = SystemTime::now();
    FileAttr {
        ino: INodeNo(ino),
        size: 0,
        blocks: 0,
        atime: now,
        mtime: now,
        ctime: now,
        crtime: now,
        kind: FileType::Directory,
        perm: 0o755,
        nlink: 2,
        uid,
        gid,
        rdev: 0,
        blksize: 512,
        flags: 0,
    }
}

/// The one real byte representation of a node's content this module ever serves — `getattr`'s
/// reported `size` and `read`'s actual data must derive from the exact same bytes, or the kernel
/// (which trusts `getattr`'s size to decide how much to request/expect) silently truncates
/// whatever `read` returns.
fn file_bytes(node: &NodeRecord) -> Vec<u8> {
    serde_json::to_vec(&node.metadata).unwrap_or_default()
}

fn file_attr(ino: u64, node: &NodeRecord, uid: u32, gid: u32) -> FileAttr {
    let size = file_bytes(node).len() as u64;
    let mtime = UNIX_EPOCH + Duration::from_secs(node.updated_at);
    let crtime = UNIX_EPOCH + Duration::from_secs(node.created_at);
    FileAttr {
        ino: INodeNo(ino),
        size,
        blocks: size.div_ceil(512).max(1),
        atime: mtime,
        mtime,
        ctime: mtime,
        crtime,
        kind: FileType::RegularFile,
        perm: 0o644,
        nlink: 1,
        uid,
        gid,
        rdev: 0,
        blksize: 512,
        flags: 0,
    }
}

/// A stateless `ino <-> synthesized path` bijection. Stateless in the sense that it never needs
/// to be told about a deletion or a rename — every mapping it ever hands out remains a valid,
/// stable label for that path for the mount's whole lifetime (FUSE inode numbers are permitted to
/// keep referring to a path that no longer resolves; the read paths below simply return `ENOENT`
/// for those). Root is the one path that always exists and is never itself a Knowledge Graph
/// object: fixed at ino 1, mapped to the empty-string path.
struct InodeTable {
    next: AtomicU64,
    by_ino: Mutex<HashMap<u64, String>>,
    by_path: Mutex<HashMap<String, u64>>,
}

impl InodeTable {
    fn new() -> Self {
        let by_ino = HashMap::from([(ROOT_INO, String::new())]);
        let by_path = HashMap::from([(String::new(), ROOT_INO)]);
        InodeTable {
            next: AtomicU64::new(ROOT_INO + 1),
            by_ino: Mutex::new(by_ino),
            by_path: Mutex::new(by_path),
        }
    }

    fn ino_for(&self, path: &str) -> u64 {
        if let Some(&ino) = self.by_path.lock().unwrap().get(path) {
            return ino;
        }
        let ino = self.next.fetch_add(1, Ordering::Relaxed);
        self.by_ino.lock().unwrap().insert(ino, path.to_string());
        self.by_path.lock().unwrap().insert(path.to_string(), ino);
        ino
    }

    fn path_for(&self, ino: u64) -> Option<String> {
        self.by_ino.lock().unwrap().get(&ino).cloned()
    }
}

/// The real `fuser::Filesystem` adapter over one [`SemanticFilesystem`], mounted under one fixed
/// capability identity — see this module's own doc comment for why that's a real, named scope
/// boundary rather than a shortcut.
pub struct PosixMount {
    fs: Arc<SemanticFilesystem>,
    monitor: CapabilityMonitor,
    token: CapabilityToken,
    inodes: InodeTable,
    next_fh: AtomicU64,
    /// Bytes accumulated for a file handle opened for writing, keyed by that handle — FUSE
    /// delivers a write as a sequence of possibly-partial, possibly-out-of-order `write()` calls;
    /// [`SemanticFilesystem::write_back`] takes one complete `serde_json::Value`, so this buffers
    /// until `release()` closes the handle.
    write_buffers: Mutex<HashMap<u64, Vec<u8>>>,
    /// The synthesized path a write-open file handle will `write_back` to on release.
    fh_paths: Mutex<HashMap<u64, String>>,
}

impl PosixMount {
    fn new(
        fs: Arc<SemanticFilesystem>,
        monitor: CapabilityMonitor,
        token: CapabilityToken,
    ) -> Self {
        PosixMount {
            fs,
            monitor,
            token,
            inodes: InodeTable::new(),
            next_fh: AtomicU64::new(1),
            write_buffers: Mutex::new(HashMap::new()),
            fh_paths: Mutex::new(HashMap::new()),
        }
    }

    fn all_paths(&self) -> Result<Vec<DirEntry>, ()> {
        self.fs
            .all_paths(&self.monitor, &self.token)
            .map_err(|_| ())
    }
}

impl Filesystem for PosixMount {
    fn lookup(&self, req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let (Some(name), Some(parent_path)) = (name.to_str(), self.inodes.path_for(parent.0))
        else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let Ok(entries) = self.all_paths() else {
            reply.error(fuser::Errno::EIO);
            return;
        };
        let children = children_of(&parent_path, &entries);
        let Some(kind) = children.get(name) else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let full = full_path(&parent_path, name);
        let ino = self.inodes.ino_for(&full);
        match kind {
            ChildKind::Directory => {
                reply.entry(&TTL, &dir_attr(ino, req.uid(), req.gid()), Generation(0));
            }
            ChildKind::File(id) => match self.fs.get_object(&self.monitor, &self.token, *id) {
                Ok(node) => reply.entry(
                    &TTL,
                    &file_attr(ino, &node, req.uid(), req.gid()),
                    Generation(0),
                ),
                Err(_) => reply.error(fuser::Errno::ENOENT),
            },
        }
    }

    fn getattr(&self, req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        if ino.0 == ROOT_INO {
            reply.attr(&TTL, &dir_attr(ROOT_INO, req.uid(), req.gid()));
            return;
        }
        let Some(path) = self.inodes.path_for(ino.0) else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let Ok(entries) = self.all_paths() else {
            reply.error(fuser::Errno::EIO);
            return;
        };
        let has_children = entries
            .iter()
            .any(|e| e.path.strip_prefix(&format!("{path}/")).is_some());
        if has_children {
            reply.attr(&TTL, &dir_attr(ino.0, req.uid(), req.gid()));
            return;
        }
        let Some(entry) = entries.iter().find(|e| e.path == path) else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        match self
            .fs
            .get_object(&self.monitor, &self.token, entry.object_id)
        {
            Ok(node) => reply.attr(&TTL, &file_attr(ino.0, &node, req.uid(), req.gid())),
            Err(_) => reply.error(fuser::Errno::ENOENT),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let Some(path) = self.inodes.path_for(ino.0) else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let Ok(entries) = self.all_paths() else {
            reply.error(fuser::Errno::EIO);
            return;
        };

        let mut listing: Vec<(String, FileType, u64)> = vec![
            (".".to_string(), FileType::Directory, ino.0),
            ("..".to_string(), FileType::Directory, ino.0),
        ];
        for (name, kind) in children_of(&path, &entries) {
            let child_ino = self.inodes.ino_for(&full_path(&path, &name));
            let file_type = match kind {
                ChildKind::Directory => FileType::Directory,
                ChildKind::File(_) => FileType::RegularFile,
            };
            listing.push((name, file_type, child_ino));
        }

        for (i, (name, file_type, child_ino)) in
            listing.into_iter().enumerate().skip(offset as usize)
        {
            if reply.add(INodeNo(child_ino), (i + 1) as u64, file_type, &name) {
                break;
            }
        }
        reply.ok();
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let Some(path) = self.inodes.path_for(ino.0) else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let Ok(entries) = self.all_paths() else {
            reply.error(fuser::Errno::EIO);
            return;
        };
        let Some(entry) = entries.iter().find(|e| e.path == path) else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let node = match self
            .fs
            .get_object(&self.monitor, &self.token, entry.object_id)
        {
            Ok(node) => node,
            Err(_) => {
                reply.error(fuser::Errno::ENOENT);
                return;
            }
        };
        let bytes = file_bytes(&node);
        let offset = offset as usize;
        if offset >= bytes.len() {
            reply.data(&[]);
            return;
        }
        let end = (offset + size as usize).min(bytes.len());
        reply.data(&bytes[offset..end]);
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let Some(path) = self.inodes.path_for(ino.0) else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let fh = self.next_fh.fetch_add(1, Ordering::Relaxed);
        let writable = flags.0 & (libc::O_WRONLY | libc::O_RDWR) != 0;
        if writable {
            self.fh_paths.lock().unwrap().insert(fh, path);
            self.write_buffers.lock().unwrap().insert(fh, Vec::new());
        }
        reply.opened(FileHandle(fh), FopenFlags::empty());
    }

    fn write(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let mut buffers = self.write_buffers.lock().unwrap();
        let Some(buf) = buffers.get_mut(&fh.0) else {
            reply.error(fuser::Errno::EBADF);
            return;
        };
        let offset = offset as usize;
        if buf.len() < offset + data.len() {
            buf.resize(offset + data.len(), 0);
        }
        buf[offset..offset + data.len()].copy_from_slice(data);
        reply.written(data.len() as u32);
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let buf = self.write_buffers.lock().unwrap().remove(&fh.0);
        let path = self.fh_paths.lock().unwrap().remove(&fh.0);
        if let (Some(buf), Some(path)) = (buf, path) {
            let metadata = serde_json::from_slice::<serde_json::Value>(&buf).unwrap_or_else(
                |_| serde_json::json!({ "text": String::from_utf8_lossy(&buf).into_owned() }),
            );
            let _ = self
                .fs
                .write_back(&self.monitor, &self.token, &path, metadata);
        }
        reply.ok();
    }

    fn create(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let (Some(name), Some(parent_path)) = (name.to_str(), self.inodes.path_for(parent.0))
        else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let full = full_path(&parent_path, name);
        let id = match self
            .fs
            .write_back(&self.monitor, &self.token, &full, serde_json::json!({}))
        {
            Ok(id) => id,
            Err(_) => {
                reply.error(fuser::Errno::EACCES);
                return;
            }
        };
        let node = match self.fs.get_object(&self.monitor, &self.token, id) {
            Ok(node) => node,
            Err(_) => {
                reply.error(fuser::Errno::EIO);
                return;
            }
        };
        let ino = self.inodes.ino_for(&full);
        let fh = self.next_fh.fetch_add(1, Ordering::Relaxed);
        self.fh_paths.lock().unwrap().insert(fh, full);
        self.write_buffers.lock().unwrap().insert(fh, Vec::new());
        reply.created(
            &TTL,
            &file_attr(ino, &node, req.uid(), req.gid()),
            Generation(0),
            FileHandle(fh),
            FopenFlags::empty(),
        );
    }

    fn mkdir(
        &self,
        req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let (Some(name), Some(parent_path)) = (name.to_str(), self.inodes.path_for(parent.0))
        else {
            reply.error(fuser::Errno::ENOENT);
            return;
        };
        let parent_id = if parent_path.is_empty() {
            None
        } else {
            self.fs
                .resolve_path(&self.monitor, &self.token, &parent_path)
                .ok()
        };
        let id = match self
            .fs
            .mkcollection(&self.monitor, &self.token, name, parent_id)
        {
            Ok(id) => id,
            Err(_) => {
                reply.error(fuser::Errno::EACCES);
                return;
            }
        };
        // A real Collection is confirmed to exist (`mkcollection` succeeded) before replying --
        // `get_object` failing here would mean the write we just made isn't visible to our own
        // very next read, which would be a real, separate bug worth surfacing as EIO rather than
        // silently reporting success for a directory that may not be readable.
        if self.fs.get_object(&self.monitor, &self.token, id).is_err() {
            reply.error(fuser::Errno::EIO);
            return;
        }
        let full = full_path(&parent_path, name);
        let ino = self.inodes.ino_for(&full);
        // A directory reply must report a directory-kind attribute -- the kernel treats a
        // `mkdir` reply describing anything else as invalid and surfaces `EIO` to the caller
        // even though the underlying Collection was created successfully (a real, previously
        // undiagnosed bug in this module, caught by `tests/posix_mount.rs`'s own real, mounted
        // `mkdir` exercise). This module's own doc comment already names why a Collection's real
        // per-object attributes aren't surfaced on its directory inode either way.
        reply.entry(&TTL, &dir_attr(ino, req.uid(), req.gid()), Generation(0));
    }
}

/// docs/10's own previously-named-missing `fs.mount_posix` — blocks the calling thread until the
/// mount is unmounted (`fusermount -u`, or the mounting process exiting). See [`spawn_mount_posix`]
/// for a non-blocking handle-returning variant.
pub fn mount_posix(
    fs: Arc<SemanticFilesystem>,
    monitor: CapabilityMonitor,
    token: CapabilityToken,
    mountpoint: impl AsRef<Path>,
) -> io::Result<()> {
    let mount = PosixMount::new(fs, monitor, token);
    let mut config = Config::default();
    config.mount_options = vec![MountOption::FSName("hyperion-semantic-fs".to_string())];
    fuser::mount2(mount, mountpoint.as_ref(), &config)
}

/// The non-blocking counterpart of [`mount_posix`]: mounts on a background thread and returns a
/// handle that unmounts on drop, so a caller (e.g. a test, or a real Hyperion session that wants
/// to keep doing other work) never has to dedicate its own thread to the mount's event loop.
pub fn spawn_mount_posix(
    fs: Arc<SemanticFilesystem>,
    monitor: CapabilityMonitor,
    token: CapabilityToken,
    mountpoint: impl AsRef<Path>,
) -> io::Result<BackgroundSession> {
    let mount = PosixMount::new(fs, monitor, token);
    let mut config = Config::default();
    config.mount_options = vec![MountOption::FSName("hyperion-semantic-fs".to_string())];
    fuser::spawn_mount2(mount, mountpoint.as_ref(), &config)
}
