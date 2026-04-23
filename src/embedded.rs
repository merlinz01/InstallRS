//! Types for the compile-time embedded-entries table plus the runtime payload
//! integrity check.

use anyhow::{anyhow, Result};

/// Verify the compressed-payload SHA-256 emitted by the build tool. `blobs`
/// is a flat list of the unique embedded byte slices (one per deduplicated
/// storage file, in `D_*` declaration order); `uninstaller_data` is the
/// embedded uninstaller (empty slice for an uninstaller binary). Call at
/// process start — the generated `main()` invokes it before anything else.
pub fn verify_payload(blobs: &[&[u8]], uninstaller_data: &[u8], expected: &[u8; 32]) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for b in blobs {
        h.update(b);
    }
    h.update(uninstaller_data);
    let got: [u8; 32] = h.finalize().into();
    if &got != expected {
        return Err(anyhow!(
            "installer payload integrity check failed — the file may be corrupt or tampered with"
        ));
    }
    Ok(())
}

/// A top-level embedded entry baked into the installer binary at compile time.
pub enum EmbeddedEntry {
    File {
        source_path_hash: u64,
        data: &'static [u8],
        compression: &'static str,
    },
    Dir {
        source_path_hash: u64,
        children: &'static [DirChild],
    },
}

/// A named child inside an [`EmbeddedEntry::Dir`] tree.
pub struct DirChild {
    pub name: &'static str,
    pub kind: DirChildKind,
}

/// The payload of a [`DirChild`] — either file data or a nested directory.
pub enum DirChildKind {
    File {
        data: &'static [u8],
        compression: &'static str,
    },
    Dir {
        children: &'static [DirChild],
    },
}
