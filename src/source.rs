//! Compile-time path-hashing and the [`source!`] macro.

/// Compile-time FNV-1a 64-bit hash of a path string (backslashes normalized to forward slashes).
///
/// Used by the [`crate::source!`] macro.
#[doc(hidden)]
pub const fn source_path_hash_const(path: &str) -> u64 {
    let bytes = path.as_bytes();
    let mut h: u64 = 14695981039346656037;
    let mut i = 0;
    while i < bytes.len() {
        let b = if bytes[i] == b'\\' { b'/' } else { bytes[i] };
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
        i += 1;
    }
    h
}

/// Compile-time reference to an embedded file or directory.
///
/// Create one with the [`crate::source!`] macro, then pass it to
/// [`crate::Installer::file`] or [`crate::Installer::dir`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Source(#[doc(hidden)] pub u64);

/// Produce a [`Source`] from a literal path, hashed at compile time.
///
/// The path string itself never appears in the final binary. Paths are
/// relative to the project root as seen by the build tool.
///
/// Accepts optional build-time-only keyword arguments; the values are parsed
/// by the build tool's scanner (not used at runtime). Supported keys:
///
/// - `ignore = ["glob", ...]` — extra glob patterns applied when gathering a
///   directory, merged with the CLI-level `--ignore` list.
/// - `features = ["name", ...]` — cargo-feature gate. The source is only
///   embedded when at least one of the listed features is enabled via
///   `installrs --feature <name>`.
///
/// ```rust,ignore
/// i.file(installrs::source!("assets/config.toml"), "etc/myapp/config.toml")
///     .install()?;
/// i.dir(source!("assets", ignore = ["*.bak", "scratch"]), "assets").install()?;
/// i.file(source!("pro.dat", features = ["pro"]), "pro.dat").install()?;
/// ```
#[macro_export]
macro_rules! source {
    ($path:literal $(, $key:ident = $val:expr)* $(,)?) => {{
        const H: u64 = $crate::source_path_hash_const($path);
        $crate::Source(H)
    }};
}
