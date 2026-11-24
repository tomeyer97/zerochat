#!/bin/sh
# Checks that node and npm are on PATH and that node's version is in the one supported range
# (web/package.json "engines.node"). Run first by every make target that uses npm.
# Exits 1 with the required range and how to get it; prints nothing when all is well.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
# The range, e.g. `^22.22.2 || ^24.15.0 || >=26.0.0`, from the "node" line under "engines".
range=$(sed -n '/"engines"/,/}/s/.*"node": *"\([^"]*\)".*/\1/p' "$root/web/package.json")
if [ -z "$range" ]; then
  echo "check-node: no engines.node in web/package.json" >&2
  exit 1
fi

fail() {
  {
    echo "zerochat needs Node.js $range with npm; $1."
    echo "Get it from https://nodejs.org/ or with nvm (https://github.com/nvm-sh/nvm): \`nvm install\` reads .nvmrc."
    echo "A distribution's packaged Node is often too old (Ubuntu 24.04 ships 18)."
  } >&2
  exit 1
}

command -v node >/dev/null 2>&1 || fail "node was not found on PATH"
command -v npm >/dev/null 2>&1 || fail "npm was not found on PATH"

found=$(node --version 2>/dev/null | head -n 1 | sed 's/^v//')
# x.y.z (a pre-release or build suffix is ignored).
core=$(printf '%s' "$found" | sed 's/[-+].*//')
case "$core" in
  *[!0-9.]* | "" | *..* | .* | *.) fail "found node ${found:-with no version}" ;;
esac
major=$(printf '%s' "$core" | cut -d. -f1)
minor=$(printf '%s' "$core" | cut -s -d. -f2)
patch=$(printf '%s' "$core" | cut -s -d. -f3)
: "${minor:=0}" "${patch:=0}"

# 0 when x.y.z (the found version) >= a.b.c.
at_least() {
  [ "$major" -gt "$1" ] && return 0
  [ "$major" -lt "$1" ] && return 1
  [ "$minor" -gt "$2" ] && return 0
  [ "$minor" -lt "$2" ] && return 1
  [ "$patch" -ge "$3" ]
}

# Each alternative is `^a.b.c` (same major, at least a.b.c) or `>=a.b.c`.
alternatives=$(printf '%s' "$range" | tr '|' ' ')
ok=1
for alt in $alternatives; do
  op=$(printf '%s' "$alt" | sed 's/[0-9.].*//')
  v=$(printf '%s' "$alt" | sed 's/^[^0-9]*//')
  a=$(printf '%s' "$v" | cut -d. -f1)
  b=$(printf '%s' "$v" | cut -s -d. -f2)
  c=$(printf '%s' "$v" | cut -s -d. -f3)
  : "${b:=0}" "${c:=0}"
  case "$op" in
    '^') [ "$major" -eq "$a" ] && at_least "$a" "$b" "$c" && ok=0 ;;
    '>=') at_least "$a" "$b" "$c" && ok=0 ;;
    *) echo "check-node: cannot read the range part '$alt' in web/package.json" >&2; exit 1 ;;
  esac
done
[ "$ok" -eq 0 ] || fail "found node $found"
