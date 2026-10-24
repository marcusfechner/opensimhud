#!/usr/bin/env bash
# Release a new version: ./scripts/release.sh 0.2.0
# Sets the version, commits, tags and pushes; .github/workflows/release.yml then builds,
# signs and publishes it, and installed apps offer the update.
set -euo pipefail

version=${1:?usage: scripts/release.sh X.Y.Z}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Version must look like 1.2.3" >&2; exit 1; }
cd "$(dirname "$0")/.."
[[ -z $(git status --porcelain) ]] || { echo "Commit or stash your changes first" >&2; exit 1; }
[[ $(git branch --show-current) == main ]] || { echo "Release from main" >&2; exit 1; }

# the app's version comes from Cargo.toml; package.json follows along
sed -i '' -E "/^\[workspace.package\]/,/^\[/ s/^version = \".*\"/version = \"$version\"/" Cargo.toml
(cd app && npm version "$version" --no-git-tag-version --allow-same-version >/dev/null)
cargo check -q -p opensimhud

git add Cargo.toml Cargo.lock app/package.json app/package-lock.json
git commit -q -m "Release $version"
git tag "v$version"
git push -q origin main "v$version"
echo "Pushed v$version. The release appears once the Release workflow finishes."
