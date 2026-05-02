<!-- markdownlint-configure-file { "MD013": { "line_length": 100 } } -->

# Building installers for production

This guide covers everything that happens between `installrs build .
--output my-installer` and a binary you can hand to users: cross-compilation
to other platforms, tuning the compile/size tradeoffs, reproducibility
and integrity, code signing, and CI release patterns.

## How the build works

When you run `installrs build <dir> --output <file>`, the CLI:

1. Reads your installer crate's `Cargo.toml` and verifies the `installrs`
   version requirement is compatible with the CLI's own version. A mismatch
   errors out here — not deep in cargo's output later.
2. Scans your source files for `source!("path")` macro invocations and
   builds a list of files / directories to embed.
3. Compresses each embedded file (default LZMA) into a content-addressed
   cache under `build/`.
4. Generates two Rust crates under `build/installer/` and `build/uninstaller/`:
   - `main.rs` with `static ENTRIES: &[EmbeddedEntry] = &[...]`,
     `include_bytes!`-loaded byte slices, and a SHA-256 `PAYLOAD_HASH`
     checked at startup.
   - `Cargo.toml` pinning `installrs = "=<CLI version>"` so the runtime
     matches exactly what the CLI was built from.
5. Runs `cargo build --release` inside each generated crate.
6. Embeds the compiled uninstaller binary into the installer via
   `include_bytes!`, then re-compiles the installer.
7. Copies the final installer binary to `--output`.

The generated release profile is aggressive:

```toml
[profile.release]
opt-level = "z"
strip = true
lto = true
codegen-units = 1
```

That's small-binary, slow-compile territory — appropriate for a
production installer you ship once, painful for an iterate-fast
development loop. See [Fast iteration](#fast-iteration) below.

## Cross-compilation

The CLI itself runs on your host, but the installer it produces can
target any Rust-supported platform. Pass `--target` to specify the
Rust target triple.

### Windows installer from Linux

```sh
rustup target add x86_64-pc-windows-gnu
installrs build . --output installer.exe --target x86_64-pc-windows-gnu
```

The `.exe` extension is added automatically when the target triple
contains `windows` and the output path has no extension.

Windows resources (icon, VERSIONINFO, manifest) from
`[package.metadata.installrs]` apply on this cross-target path.
PNG-to-ICO conversion runs on the Linux host — no Windows tooling
needed.

### Linux installer from Windows

```sh
rustup target add x86_64-unknown-linux-gnu
installrs build . --output installer --target x86_64-unknown-linux-gnu
```

**Gotcha:** cross-compiling a GTK installer from non-Linux is not well
supported by `gtk-rs`. Build Linux installers on Linux.

### macOS

```sh
rustup target add aarch64-apple-darwin    # Apple Silicon
rustup target add x86_64-apple-darwin     # Intel Macs
installrs build . --output installer --target aarch64-apple-darwin
```

macOS builds don't currently have a supported wizard GUI — installers
run in console mode or with your own custom UI. PRs welcome.

### Picking compression for cross-compiled builds

- **LZMA** (default) — best compression, slower to build.
- **gzip** — moderate compression, fast build.
- **bzip2** — better than gzip, slightly worse than LZMA.
- **none** — no compression. Fastest build, largest binary. Useful for
  iteration; rarely what you want in production.

```sh
installrs build . --output installer --compression gzip
```

## Compile / size tradeoffs

### Fast iteration

Override the generated crates' release profile via cargo env vars —
they're inherited by the inner `cargo build --release`:

```sh
CARGO_PROFILE_RELEASE_LTO=false \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
CARGO_PROFILE_RELEASE_OPT_LEVEL=1 \
installrs build .
```

Expect a 3–5× speedup at the cost of a larger binary. Drop these for
the real release build.

### Production-size binary

The defaults already optimize for size. If you still want to squeeze:

1. **Use LZMA compression.** `--compression lzma` (the default) gets
   significantly smaller archives than gzip or bzip2 for typical
   text/binary mixes.
2. **Minimize embedded dependencies.** If you're pulling in large
   crates from your installer lib just for convenience, the bloat is
   yours to trim — `installrs` only embeds what you reference.
3. **Don't ship debug info.** `strip = true` in the generated profile
   already handles this, but confirm you're not overriding it.
4. **Minimize icon sizes.** If you only need 16×16, 32×32, and 48×48
   frames, set `icon-sizes = [16, 32, 48]` in
   `[package.metadata.installrs]` — the default pulls in 128×128 and
   256×256 which are hundreds of KB each.
5. **Trim CI-only features.** If your installer doesn't use `bzip2`,
   disable the feature globally in your installer crate's Cargo.toml
   so that version of the runtime doesn't get linked in.

### Build speed for CI

Both the CLI and the generated installer crate's builds benefit from
caching. In GitHub Actions:

- `Swatinem/rust-cache@v2` caches `target/` for your installer crate.
- Extend `workspaces:` to cover the generated crates too — they have
  their own `target/` dirs under `build/installer/` and
  `build/uninstaller/`.
- Use the fast-iteration env vars above on CI validation jobs that
  don't need to ship the binary.

Example:

```yaml
- uses: Swatinem/rust-cache@v2
  with:
    workspaces: |
      . -> target
      example/build/installer -> target
      example/build/uninstaller -> target

- name: Build example installer (fast)
  env:
    CARGO_PROFILE_RELEASE_LTO: "false"
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "16"
    CARGO_PROFILE_RELEASE_OPT_LEVEL: "1"
  run: installrs build example --output installer-ci
```

## Integrity and reproducibility

### Payload hash

Every generated installer embeds `PAYLOAD_HASH: [u8; 32]` — a SHA-256
over the compressed embedded file blobs plus the uninstaller binary.
The generated `main()` calls `installrs::verify_payload(...)` before
anything else; on mismatch the installer prints an error and exits 1.

This catches accidental corruption (download truncation, bit rot) but
does **not** protect against an attacker who rebuilds the installer
from source. For that, you need code signing — see below.

### Reproducible builds

With the exact-version runtime pin (`installrs = "=X.Y.Z"` in the
generated `Cargo.toml`), a given CLI version always links the same
runtime version. For full bit-for-bit reproducibility, also:

1. **Pin Rust.** Add `rust-toolchain.toml` to your installer crate with
   the exact toolchain used for releases. Without this, different
   `rustc` versions on different machines produce different binaries.
2. **Commit `Cargo.lock`** in your installer crate so dependency
   versions are pinned.
3. **Install the CLI with `--locked`** (`cargo install installrs
--locked`) so its own transitive deps come from the published
   lockfile.
4. **Use `SOURCE_DATE_EPOCH`** to eliminate build timestamps from the
   binary if you care about byte-for-byte matching.
5. **Same compression flag.** `--compression lzma` is deterministic;
   all supported methods are, but make sure the build script is
   consistent.

## Code signing

The payload hash verifies the embedded files are intact, but Windows
SmartScreen and macOS Gatekeeper care about the whole executable being
signed by a recognized certificate. InstallRS doesn't sign anything
itself — signing happens **after** `installrs` produces the binary.

### Windows (Authenticode)

```sh
signtool sign /fd SHA256 /f cert.pfx /p <password> \
  /tr http://timestamp.digicert.com /td SHA256 \
  /d "My App Installer" my-installer.exe
```

An EV (Extended Validation) certificate avoids SmartScreen warnings;
a regular code-signing cert requires reputation to build up first.

## Release CI patterns

A typical release workflow:

1. **Tag** a version (`git tag v1.2.3 && git push --tags`).
2. **Publish the runtime crate** to crates.io first (the generated
   installer crates resolve their `installrs = "=<version>"` dep from
   the index).
3. **Build installers** for every target you support from the published
   crate version — `cargo install installrs --locked --version <tag>
--target <triple>`. Building from the just-published crate
   double-checks it's installable.
4. **Sign** the binaries.
5. **Package** into `.tar.gz` (Unix) or `.zip` (Windows) so `cargo
binstall` and similar tools auto-detect them.
6. **Hash** (SHA-256 sidecar) each artifact.
7. **Upload** to GitHub Releases.

See this repo's [`.github/workflows/release.yml`](../.github/workflows/release.yml)
for a working template — it handles crates.io OIDC trusted publishing,
matrix-builds Linux + Windows binaries, generates SHA-256 sidecars,
and uploads everything to the Releases page in a single tag-triggered
run.

## Troubleshooting

### "package `installrs` not found in registry"

You're trying to build without the runtime crate published yet, or on
an offline build host. For local development of InstallRS itself, pass
`--installrs-path <PATH>` to emit `installrs = { path = "<PATH>" }` in
the generated `Cargo.toml` instead of a crates.io reference.

### "installer payload integrity check failed"

The binary has been tampered with or corrupted. Re-download, or
rebuild from source. If this happens on a fresh build from a clean
tree, that's a bug — please file an issue.

### Generated binary is way bigger than expected

- Check `icon-sizes` — the 256×256 frame is large.
- Check your installer crate's dep graph with `cargo bloat
--release --crates` (run in the generated `build/installer/`
  directory). Often a single large dep dominates.
- Ensure `opt-level = "z"` and `strip = true` are still active — your
  env-var overrides from iteration might be lingering.

## See also

- [Builder CLI reference](builder-cli.md) — every flag the `installrs`
  command accepts.
- [Windows Resources](windows-resources.md) — icon, manifest, and
  VERSIONINFO config relevant to the Windows cross-compile path.
- [Embedded files, builder ops, and progress](embedded-files.md) —
  `--compression` choice has a big impact on build time and binary
  size; see the compression tradeoffs above plus this doc for runtime
  decompression details.
