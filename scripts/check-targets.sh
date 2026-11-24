#!/usr/bin/env bash
# `cargo check --target` for the release targets this machine cannot build natively.
#
#   scripts/check-targets.sh [target...]   (default: x86_64-pc-windows-gnu x86_64-apple-darwin aarch64-apple-darwin)
#
# Needs only the target's std (`rustup target add <target>`). Without a C cross compiler for the
# target, the C parts of dependencies (bundled SQLite, ring) are not compiled: a stub stands in for
# the compiler and archiver, so this checks the Rust code only. It prints one line per target.
set -uo pipefail
cd "$(dirname "$0")/.."

targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(x86_64-pc-windows-gnu x86_64-apple-darwin aarch64-apple-darwin)

dir=target/check-targets
mkdir -p "$dir"
stub="$PWD/$dir/stub-cc"
cat > "$stub" <<'STUB'
#!/bin/sh
# Stands in for a missing C cross compiler or archiver: writes an empty output file.
out=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done
[ -n "$out" ] && : > "$out"
exit 0
STUB
chmod +x "$stub"

installed=$(rustup target list --installed 2>/dev/null)
status=0
for t in "${targets[@]}"; do
  if ! grep -qx "$t" <<<"$installed"; then
    echo "$t: skipped (rustup target add $t)"
    continue
  fi
  cc_var="CC_${t//-/_}"; ar_var="AR_${t//-/_}"
  note=""
  if [ -z "${!cc_var:-}" ]; then
    export "$cc_var=$stub" "$ar_var=$stub"
    note=" (C code stubbed: no C cross compiler)"
  fi
  log="$dir/$t.log"
  if cargo check --workspace --all-targets --target "$t" --target-dir "$dir" >"$log" 2>&1; then
    echo "$t: ok$note"
  else
    echo "$t: FAILED$note: $(grep -c '^error' "$log") errors, see $log"
    status=1
  fi
  unset "$cc_var" "$ar_var"
done
exit $status
