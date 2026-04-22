# AGENTS.md

Guidance for AI coding tools working on this project.

@README.md
@docs/getting-started.md
@docs/architecture.md

## Commands

```bash
# Build library and binary
cargo build --release

# Run tests
cargo test

# Check without building
cargo check

# Lint - always run before committing
cargo clippy

# Cross-check the Win32 backend from Linux (requires gcc-mingw-w64-x86-64
# and `rustup target add x86_64-pc-windows-gnu`)
cargo clippy --features gui-win32 --target x86_64-pc-windows-gnu

# Format - always run before committing
cargo fmt

# Build an installer using the CLI (from an installer source directory)
installrs --target ./my-installer --output installer

# Build against the working-tree runtime (not the published crate) —
# set this for InstallRS-on-InstallRS development.
INSTALLRS_LOCAL_PATH=1 installrs --target ./my-installer --output installer

# Verbose / trace output
installrs --target ./my-installer -v    # debug
installrs --target ./my-installer -vv   # trace
```

## Keeping docs in sync

When you change code that affects how someone uses or understands
InstallRS, update README.md and/or the relevant doc files in `docs/`.
Make sure to update documentation indexes and links.
If you add a new doc file, add it to any relevant indexes
and link to it from the README and any relevant "See also" doc sections.
Also update `CHANGELOG.md` with a brief note about the change,
following the format of previous entries, keeping it concise but informative.
