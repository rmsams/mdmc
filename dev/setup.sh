#!/usr/bin/env bash
# Set up the local mdmc dev environment: copy the committed fixtures from
# dev/ into .dev-state/ (so testing mutates a git-ignored working copy, never
# the committed assets), then index it with notmuch and mdmc.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE="$ROOT/.dev-state"

rm -rf "$STATE"
mkdir -p "$STATE"

cp -r "$ROOT/dev/mail" "$STATE/mail"
cp -r "$ROOT/dev/drafts" "$STATE/drafts"
cp "$ROOT/dev/mdmc.toml" "$STATE/mdmc.toml"

cat > "$STATE/notmuch-config" <<EOF
[database]
path=$STATE/mail

[new]
ignore=.uidvalidity;.mbsyncstate
tags=new;unread

[user]
name=Dev Tester
primary_email=dev@example.com
EOF

export NOTMUCH_CONFIG="$STATE/notmuch-config"

notmuch new
cargo run --manifest-path "$ROOT/Cargo.toml" -- --config "$STATE/mdmc.toml" index

echo "dev environment ready at $STATE (run 'dev-teardown' to reset)"
