#!/usr/bin/env bash
#
# Automates an InstallRS release: version bump, CHANGELOG rotation,
# lint/test, commit, tag, push. Push to origin triggers the release
# workflow (crates.io + GitHub release + binaries).
#
# Usage: ./scripts/release.sh

set -euo pipefail

cd "$(dirname "$0")/.."

red()    { printf '\033[31m%s\033[0m\n' "$*" >&2; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
bold()   { printf '\033[1m%s\033[0m\n' "$*"; }

die() { red "error: $*"; exit 1; }

confirm() {
    local prompt="${1:-Continue?}"
    read -r -p "$prompt [y/N] " answer
    [[ "$answer" =~ ^[yY]([eE][sS])?$ ]]
}

# ── Preconditions ───────────────────────────────────────────────────────────

[ -f Cargo.toml ] || die "must run from the repo root (Cargo.toml not found)"
[ -d .git ] || die "not a git repo"

if [ -n "$(git status --porcelain)" ]; then
    git status --short
    die "working tree is dirty; commit or stash first"
fi

current_branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$current_branch" != "main" ]; then
    yellow "warning: not on main (current: $current_branch)"
    confirm "Continue anyway?" || exit 1
fi

current_version="$(cargo metadata --no-deps --format-version=1 \
    | python3 -c 'import sys,json; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="installrs"))')"
bold "Current version: $current_version"

# ── Tag prompt ──────────────────────────────────────────────────────────────

read -r -p "New tag name (e.g. v1.2.3): " tag
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]] \
    || die "tag must match 'vMAJOR.MINOR.PATCH[-prerelease]'"

version="${tag#v}"

if git rev-parse "$tag" >/dev/null 2>&1; then
    die "tag $tag already exists"
fi

if [ "$version" = "$current_version" ]; then
    die "version $version matches current Cargo.toml version; nothing to bump"
fi

today="$(date +%Y-%m-%d)"
bold "Releasing $tag ($version) dated $today"

# ── Cargo.toml ──────────────────────────────────────────────────────────────

sed -i -E "0,/^version = \"[^\"]+\"/s//version = \"$version\"/" Cargo.toml

# ── CHANGELOG.md ────────────────────────────────────────────────────────────

if ! grep -q "^## \[Unreleased\]" CHANGELOG.md; then
    die "CHANGELOG.md has no [Unreleased] section"
fi

# Check there's actually content between [Unreleased] and the next section.
unreleased_body="$(awk '
    /^## \[Unreleased\]/  { in_section=1; next }
    in_section && /^## \[/ { exit }
    in_section { print }
' CHANGELOG.md | sed '/^$/d')"

if [ -z "$unreleased_body" ]; then
    yellow "[Unreleased] section is empty"
    confirm "Release anyway (empty changelog)?" || exit 1
fi

# Insert new version header right after [Unreleased].
awk -v ver="$version" -v d="$today" '
    /^## \[Unreleased\]/ {
        print
        print ""
        print "## [" ver "] — " d
        inserted = 1
        next
    }
    { print }
' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

# Update compare links at bottom of CHANGELOG.
# [Unreleased] should compare new-tag...HEAD
# [new-version] should compare old-tag...new-tag
prev_tag="v$current_version"
awk -v new_tag="$tag" -v prev_tag="$prev_tag" -v ver="$version" '
    /^\[Unreleased\]:/ {
        sub(/v[^.]+\.[^.]+\.[^.]+[^.]*\.\.\.HEAD/, new_tag "...HEAD")
        print
        print "[" ver "]: https://github.com/merlinz01/InstallRS/compare/" prev_tag "..." new_tag
        next
    }
    { print }
' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

# ── Validate & sync lockfile ────────────────────────────────────────────────

bold "Running cargo fmt --check"
cargo fmt --check || die "cargo fmt --check failed"

bold "Running cargo clippy"
cargo clippy --all-targets -- -D warnings || die "clippy failed"

bold "Running cargo clippy --features gui-win32 --target x86_64-pc-windows-gnu"
if rustup target list --installed | grep -q x86_64-pc-windows-gnu; then
    cargo clippy --features gui-win32 --target x86_64-pc-windows-gnu -- -D warnings \
        || die "win32 clippy failed"
else
    yellow "skipping win32 clippy (target not installed)"
fi

bold "Running cargo test"
cargo test --release --test integration || die "integration tests failed"

bold "Running cargo build --release (lockfile sync)"
cargo build --release >/dev/null

# ── Review ──────────────────────────────────────────────────────────────────

bold "Diff to be committed:"
git --no-pager diff -- Cargo.toml Cargo.lock CHANGELOG.md
echo
confirm "Commit, tag $tag, and push to origin?" || {
    yellow "Aborting. Discarding changes with git checkout ..."
    git checkout -- Cargo.toml Cargo.lock CHANGELOG.md
    exit 1
}

# ── Commit, tag, push ───────────────────────────────────────────────────────

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "$tag"
git tag -a "$tag" -m "$tag"

bold "Pushing main and $tag to origin"
git push origin main "$tag"

green "Released $tag"
echo "  Actions:  https://github.com/merlinz01/InstallRS/actions"
echo "  Releases: https://github.com/merlinz01/InstallRS/releases/tag/$tag"
echo "  crates.io: https://crates.io/crates/installrs/$version"
