#!/usr/bin/env bash
# CI's Linux checks in Docker, without a Linux machine.
#
#   tools/linux-check/run.sh [source-dir] [command...]
#
# source-dir defaults to this repository. It's copied into the container (not mounted, so it works
# with Colima and any other VM that only shares some folders), without .git, node_modules,
# dist or src-tauri/target. With a command (e.g. `bash`) the container runs that instead of the
# checks. Cargo's target dir, its registry and npm's cache live in named volumes, so later runs
# are incremental and the host's src-tauri/target is never touched. Clean up with:
#
#   docker image rm pitwall-linux-check
#   docker volume rm pitwall-linux-check-target pitwall-linux-check-cargo pitwall-linux-check-npm
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
src=$(cd "${1:-$here/../..}" && pwd)
shift || true

image=pitwall-linux-check
docker build --tag "$image" "$here"

tty=()
[ -t 0 ] && [ -t 1 ] && tty=(--interactive --tty)

container=$(docker create ${tty[@]+"${tty[@]}"} \
  --volume pitwall-linux-check-target:/cargo-target \
  --volume pitwall-linux-check-cargo:/usr/local/cargo/registry \
  --volume pitwall-linux-check-npm:/root/.npm \
  "$image" "$@")
trap 'docker rm --force "$container" >/dev/null' EXIT

COPYFILE_DISABLE=1 tar -C "$src" -cf - --no-xattrs --no-acls \
  --exclude=./.git --exclude=./node_modules --exclude=./dist \
  --exclude=./src-tauri/target --exclude=./src-tauri/gen \
  . | docker cp --quiet - "$container:/src"

if ((${#tty[@]})); then
  docker start --attach --interactive "$container"
else
  docker start --attach "$container"
fi
