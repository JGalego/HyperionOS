//! A real, mounted FUSE filesystem, driven entirely through `std::fs` — proving
//! [`hyperion_semantic_fs::posix::spawn_mount_posix`] genuinely satisfies real VFS syscalls via
//! the kernel's own FUSE driver, not merely that the adapter compiles against `fuser`'s trait.
//! Every assertion here goes through a real mountpoint on disk, exactly the same path a real
//! `ls`/`cat`/`mkdir`/shell redirection would take.
//!
//! Sessions are unmounted by dropping the [`fuser::BackgroundSession`] handle rather than calling
//! its own `umount_and_join` — that's the mechanism its own doc comment names ("if it's dropped,
//! the filesystem will be unmounted"), and this hosted sandbox's direct, non-root `umount(2)`
//! syscall (the path `umount_and_join` takes) does not behave the way a real, privileged host's
//! does; `Drop`'s own best-effort fallback path is what actually works here, exactly the same way
//! this workspace's other hosted-simulator real/honest-scope choices already are.
//!
//! Linux-only, matching `hyperion_semantic_fs::posix`'s own `cfg(target_os = "linux")` gate (see
//! that module's doc comment for why).
#![cfg(target_os = "linux")]

use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::ContextEngine;
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_semantic_fs::posix::spawn_mount_posix;
use hyperion_semantic_fs::SemanticFilesystem;
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    CapabilityMonitor,
    hyperion_capability::CapabilityToken,
    Arc<SemanticFilesystem>,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut monitor = CapabilityMonitor::new();
    let token = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let graph = Arc::new(KnowledgeGraph::open(dir.path().join("kg.jsonl")).unwrap());
    let context = Arc::new(ContextEngine::new(graph.clone()));
    let fs = Arc::new(SemanticFilesystem::new(graph, context));
    (dir, monitor, token, fs)
}

#[test]
fn a_real_mounted_directory_lists_a_previously_written_object() {
    let (_kg_dir, monitor, token, fs) = setup();
    fs.write_back(
        &monitor,
        &token,
        "document/hello",
        json!({"title": "hello"}),
    )
    .unwrap();

    let mountpoint = tempfile::tempdir().unwrap();
    let session = spawn_mount_posix(fs, monitor, token, mountpoint.path()).unwrap();

    let root_entries: Vec<String> = std::fs::read_dir(mountpoint.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        root_entries.contains(&"document".to_string()),
        "expected a real 'document' directory entry from the kernel readdir, got {root_entries:?}"
    );

    let doc_entries: Vec<String> = std::fs::read_dir(mountpoint.path().join("document"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(doc_entries.contains(&"hello".to_string()));

    drop(session);
}

#[test]
fn reading_a_mounted_file_returns_the_real_node_metadata_as_json() {
    let (_kg_dir, monitor, token, fs) = setup();
    fs.write_back(
        &monitor,
        &token,
        "document/report",
        json!({"title": "report", "body": "quarterly numbers"}),
    )
    .unwrap();

    let mountpoint = tempfile::tempdir().unwrap();
    let session = spawn_mount_posix(fs, monitor, token, mountpoint.path()).unwrap();

    let content = std::fs::read_to_string(mountpoint.path().join("document/report")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["title"], "report");
    assert_eq!(parsed["body"], "quarterly numbers");

    drop(session);
}

#[test]
fn mkdir_through_the_kernel_creates_a_real_collection() {
    let (_kg_dir, monitor, token, fs) = setup();

    let mountpoint = tempfile::tempdir().unwrap();
    let session = spawn_mount_posix(fs.clone(), monitor, token, mountpoint.path()).unwrap();

    std::fs::create_dir(mountpoint.path().join("Trip")).unwrap();
    let entries: Vec<String> = std::fs::read_dir(mountpoint.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.contains(&"Trip".to_string()));

    drop(session);

    // The directory the kernel created is a real Knowledge Graph "collection" node, not a
    // filesystem-only entry — verifiable directly once the mount is gone.
    let mut fresh_monitor = CapabilityMonitor::new();
    let fresh_token = fresh_monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let id = fs
        .resolve_path(&fresh_monitor, &fresh_token, "Trip")
        .unwrap();
    let _ = id;
}

#[test]
fn writing_a_new_top_level_file_through_the_kernel_creates_a_real_object_readable_back() {
    let (_kg_dir, monitor, token, fs) = setup();

    let mountpoint = tempfile::tempdir().unwrap();
    let session = spawn_mount_posix(fs, monitor, token, mountpoint.path()).unwrap();

    // A brand new path with no existing sibling can only be created directly under the root —
    // the same real POSIX rule any other filesystem enforces: `open(O_CREAT)` needs its parent
    // directory to already resolve, and this crate's own "virtual type-prefix directory" (e.g.
    // "note/") only exists once at least one real object under it does (see `posix`'s own doc
    // comment on namespace shape).
    std::fs::write(
        mountpoint.path().join("todo"),
        br#"{"title": "todo", "items": ["milk", "eggs"]}"#,
    )
    .unwrap();

    let content = std::fs::read_to_string(mountpoint.path().join("todo")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["title"], "todo");
    assert_eq!(parsed["items"][1], "eggs");

    drop(session);
}

#[test]
fn writing_into_a_freshly_made_directory_links_it_as_a_real_collection_member() {
    let (_kg_dir, monitor, token, fs) = setup();

    let mountpoint = tempfile::tempdir().unwrap();
    let session = spawn_mount_posix(fs, monitor, token, mountpoint.path()).unwrap();

    std::fs::create_dir(mountpoint.path().join("Receipts")).unwrap();
    std::fs::write(
        mountpoint.path().join("Receipts/scan1"),
        br#"{"title": "scan1"}"#,
    )
    .unwrap();

    let entries: Vec<String> = std::fs::read_dir(mountpoint.path().join("Receipts"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.contains(&"scan1".to_string()));

    let content = std::fs::read_to_string(mountpoint.path().join("Receipts/scan1")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["title"], "scan1");

    drop(session);
}

#[test]
fn a_plain_non_json_write_is_wrapped_rather_than_rejected() {
    let (_kg_dir, monitor, token, fs) = setup();

    let mountpoint = tempfile::tempdir().unwrap();
    let session = spawn_mount_posix(fs, monitor, token, mountpoint.path()).unwrap();

    std::fs::write(mountpoint.path().join("plain"), b"just some text").unwrap();

    let content = std::fs::read_to_string(mountpoint.path().join("plain")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["text"], "just some text");

    drop(session);
}

/// Close-to-open consistency, the property POSIX programs assume without thinking about it: once
/// `close(2)` has returned, the bytes are there for the next reader.
///
/// The mount used to commit a handle's buffered bytes only on FUSE `release`, which the kernel
/// sends asynchronously and makes nothing wait for -- so `close()` could return before the write
/// had reached the graph at all, and the very next read observed the empty object `create` made.
/// The loop is deliberate: the window is small, and one round trip lands on the right side of it
/// often enough to look like a pass.
#[test]
fn a_write_is_readable_by_the_next_reader_the_instant_close_returns() {
    let (_kg_dir, monitor, token, fs) = setup();

    let mountpoint = tempfile::tempdir().unwrap();
    let session = spawn_mount_posix(fs, monitor, token, mountpoint.path()).unwrap();
    let root = mountpoint.path().to_path_buf();

    // Several writer threads rather than one: the window between `close(2)` returning and an
    // asynchronous `release` landing only opens under contention for the single-threaded FUSE
    // session, which is why the original bug surfaced on a loaded CI runner and stayed hidden
    // on an idle machine. This is a stress test, not a deterministic reproducer -- the
    // deterministic half of the fix (that a committed handle is not committed twice) is unit
    // tested in `posix.rs` instead.
    std::thread::scope(|scope| {
        for writer in 0..3 {
            let root = root.clone();
            scope.spawn(move || {
                for round in 0..50 {
                    let path = root.join(format!("note-{writer}-{round}"));
                    let written = format!("writer {writer} round {round}");

                    std::fs::write(&path, written.as_bytes()).unwrap();
                    // No sleep, no retry, no reopen: exactly what a shell pipeline or any
                    // ordinary program does immediately after a write.
                    let read_back = std::fs::read_to_string(&path).unwrap();

                    let parsed: serde_json::Value = serde_json::from_str(&read_back).unwrap();
                    assert_eq!(
                        parsed["text"], written,
                        "writer {writer} round {round}: a read issued right after close must see \
                         the written bytes, got {read_back}"
                    );
                }
            });
        }
    });

    drop(session);
}
