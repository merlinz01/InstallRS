//! Types for the compile-time embedded-entries table plus the runtime payload
//! integrity check.

use anyhow::{anyhow, Result};

/// Schema version for the contract between the `installrs` CLI's
/// generated code and this runtime crate. The CLI emits a
/// const-time assertion (`assert_entries_version`) at the top of
/// the generated `main.rs`; if the runtime ever drifts from the
/// CLI version that wrote the code, compilation fails with a clear
/// message rather than a confusing "variant not found" error deep
/// inside the generated crate.
///
/// Bump this whenever the shape of [`EmbeddedEntry`], [`DirChild`],
/// [`DirChildKind`], or the static-table layout changes in a way
/// that breaks generated code from earlier CLIs.
#[doc(hidden)]
pub const ENTRIES_VERSION: u32 = 1;

/// Compile-time check that the runtime's `ENTRIES_VERSION` matches
/// what the generating CLI expected. Called from generated `main.rs`
/// in a `const _: () = ...` slot, so a mismatch fails at compile
/// time with a panic message instead of cascading rustc errors.
#[doc(hidden)]
pub const fn assert_entries_version(generated_for: u32) {
    if generated_for != ENTRIES_VERSION {
        panic!(
            "installrs ENTRIES schema mismatch: the installrs CLI that \
             generated this crate emits version A, but the runtime \
             linked here is version B. Reinstall the matching CLI \
             (`cargo install installrs --version =<runtime-version>`) \
             or rebuild the installer crate with the matching CLI."
        );
    }
}

/// Verify the compressed-payload SHA-256 emitted by the build tool. `blobs`
/// is a flat list of the unique embedded byte slices (one per deduplicated
/// storage file, in `D_*` declaration order); `uninstaller_data` is the
/// embedded uninstaller (empty slice for an uninstaller binary). Call at
/// process start — the generated `main()` invokes it before anything else.
#[doc(hidden)]
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
#[doc(hidden)]
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
#[doc(hidden)]
pub struct DirChild {
    pub name: &'static str,
    pub kind: DirChildKind,
}

/// The payload of a [`DirChild`] — either file data or a nested directory.
#[doc(hidden)]
pub enum DirChildKind {
    File {
        data: &'static [u8],
        compression: &'static str,
    },
    Dir {
        children: &'static [DirChild],
    },
}
