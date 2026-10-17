#!/usr/bin/env bash
# Linux only: runs the renderer integration tests under strace and prints,
# for every renderer, the system calls it made after installing its seccomp
# filter and the ones the filter refused. This is how the allowlist in
# crates/sandbox/src/linux.rs is checked (docs/security.md).
set -euo pipefail

command -v strace >/dev/null || sudo apt-get install -y -qq strace >/dev/null

bin=$(cargo test -p rustlite-desktop --test renderer --no-run --message-format=json \
  | jq -r 'select(.profile.test == true and .target.name == "renderer") | .executable')
out=$(mktemp -d)
status=0
strace -f -ff -qq -o "$out/t" "$bin" --test-threads=1 || status=$?

renderers=0
for f in "$out"/t.*; do
  grep -q '^seccomp(SECCOMP_SET_MODE_FILTER' "$f" || continue
  renderers=$((renderers + 1))
  # Everything after the filter went in (the seccomp call's own line excluded).
  sed -n '/^seccomp(SECCOMP_SET_MODE_FILTER/,$p' "$f" | tail -n +2 >"$f.after"
done
echo "renderers traced: $renderers (test exit status $status)"
[ "$renderers" -gt 0 ] || exit 1

echo "== system calls after lockdown (all renderers, count name) =="
cat "$out"/t.*.after | grep -oE '^[a-z0-9_]+\(' | tr -d '(' | sort | uniq -c | sort -rn
echo "== refused by the filter (EPERM/ENOSYS), count call =="
cat "$out"/t.*.after | grep -E '= -1 (EPERM|ENOSYS)' \
  | sed -E 's/\(.*= -1 /(...) = -1 /' | sort | uniq -c | sort -rn
exit "$status"
