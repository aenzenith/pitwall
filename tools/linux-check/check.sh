#!/usr/bin/env bash
# Runs inside the pitwall-linux-check image: copies the source run.sh put in /src to /work, the
# way a fresh checkout would look, then runs CI's checks. Every step runs even when an earlier
# one fails, so one pass lists everything Linux trips over. No production build.
set -uo pipefail

# Symlinks and build output from the host (node_modules, target) must not come along, so these
# are excluded explicitly as well as through .gitignore.
rsync -a --delete \
  --exclude=/.git --exclude=/node_modules --exclude=/dist \
  --exclude=/src-tauri/target --exclude=/src-tauri/gen \
  --filter=':- .gitignore' \
  /src/ /work/ || exit 1
cd /work || exit 1

failed=()
step() {
  local name=$1
  shift
  printf '\n==> %s\n' "$name"
  if "$@"; then
    printf '<== %s: ok\n' "$name"
  else
    printf '<== %s: FAILED\n' "$name"
    failed+=("$name")
    return 1
  fi
}

if step "npm ci" npm ci --no-audit --no-fund; then
  step "vue-tsc" npx vue-tsc --noEmit
  step "vitest" npx vitest run
fi
step "clippy" cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
step "cargo test" cargo test --manifest-path src-tauri/Cargo.toml

echo
if ((${#failed[@]})); then
  echo "Linux check failed: ${failed[*]}"
  exit 1
fi
echo "Linux check passed"
