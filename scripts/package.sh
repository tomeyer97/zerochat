#!/usr/bin/env bash
# Builds the npm packages locally and shows what `npm pack` would put in each. Publishes nothing:
# releases are built and published by .github/workflows/release.yml only.
#
#   scripts/package.sh            (or `make package`)
#
# 1. builds the web UI (the server embeds web/dist at compile time),
# 2. builds zerochat and zsf in release mode for this machine. On Linux that is the static musl target
#    (x86_64/aarch64-unknown-linux-musl) when its toolchain is installed (`rustup target add <target>`
#    and musl-gcc, e.g. `apt install musl-tools`); otherwise a glibc build, which npm/build.js lays out as
#    a LOCAL TEST BUILD marked private (npm refuses to publish it) and never calls musl. Other npm
#    targets (npm/zerochat/packages.json) only when their Rust target and C cross compiler are installed,
# 3. writes THIRD_PARTY_NOTICES.md when cargo-about is installed (scripts/third-party-notices.sh),
# 4. lays out the packages in $PACKAGE_OUT (default target/npm) with npm/build.js,
# 5. runs `npm pack --dry-run` on each, a smoke test of the shims, the release completeness check
#    (it passes only with all platforms and the notices), then prints a summary.
set -euo pipefail
cd "$(dirname "$0")/.."

out="${PACKAGE_OUT:-target/npm}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"

step() { printf '\n== %s\n' "$*"; }

step "web UI"
if [ ! -f web/node_modules/.package-lock.json ] || [ web/package-lock.json -nt web/node_modules/.package-lock.json ]; then
  npm --prefix web ci --no-audit --no-fund
fi
npm --prefix web run build >/dev/null
echo "built web/dist"

# The npm platform (<platform>-<arch>) of a Rust target triple.
npm_platform() {
  local os arch
  case "$1" in
    *-linux-*) os=linux ;;
    *-apple-darwin) os=darwin ;;
    *-windows-*) os=win32 ;;
    *) return 1 ;;
  esac
  case "$1" in
    x86_64-*) arch=x64 ;;
    aarch64-*) arch=arm64 ;;
    *) return 1 ;;
  esac
  echo "$os-$arch"
}

host=$(rustc -vV | sed -n 's/^host: //p')
host_platform=$(npm_platform "$host") || { echo "this host ($host) has no npm package"; exit 1; }
installed_targets=$(rustup target list --installed 2>/dev/null || true)

step "binaries"
bin_args=()
built=()
skipped=()
notes=()
host_target=$(node npm/build.js targets | awk -v p="$host_platform" '$2 == p {print $1}')
if [[ "$host" == *-linux-* ]]; then
  # The release builds static musl: build that when the toolchain is here.
  cc_var="CC_${host_target//-/_}"
  if grep -qx "$host_target" <<<"$installed_targets" && { [ -n "${!cc_var:-}" ] || command -v musl-gcc >/dev/null; }; then
    [ -n "${!cc_var:-}" ] || export "$cc_var=musl-gcc"
    cargo build --release -p zerochat -p zsf --target "$host_target"
    bin_args+=(--bin "$host_platform=target/$host_target/release")
    built+=("$host_platform ($host_target, static, this machine)")
  else
    cargo build --release -p zerochat -p zsf
    bin_args+=(--bin "$host_platform=target/release" --allow-glibc)
    built+=("$host_platform ($host, glibc: a LOCAL TEST BUILD, private, never publishable)")
    notes+=("no musl toolchain for $host_target (rustup target add $host_target; apt install musl-tools): the $host_platform package holds a dynamically linked $host build, labelled as such")
  fi
else
  cargo build --release -p zerochat -p zsf
  bin_args+=(--bin "$host_platform=target/release")
  built+=("$host_platform ($host, this machine)")
fi

# The release targets: <triple> <platform> per line.
targets=$(node npm/build.js targets)
while read -r triple platform; do
  [ "$platform" = "$host_platform" ] && continue
  # Bundled SQLite and ring are C: a cross build needs the target's C compiler in CC_<triple>.
  cc_var="CC_${triple//-/_}"
  if ! grep -qx "$triple" <<<"$installed_targets"; then
    skipped+=("$platform ($triple): Rust target not installed (rustup target add $triple)")
  elif [ -z "${!cc_var:-}" ]; then
    skipped+=("$platform ($triple): no C cross compiler (set $cc_var)")
  else
    cargo build --release -p zerochat -p zsf --target "$triple"
    bin_args+=(--bin "$platform=target/$triple/release")
    built+=("$platform ($triple)")
  fi
done <<<"$targets"

step "third-party notices"
notices_args=()
if scripts/third-party-notices.sh "$out/THIRD_PARTY_NOTICES.md"; then
  notices_args=(--notices "$out/THIRD_PARTY_NOTICES.md")
else
  notes+=("no THIRD_PARTY_NOTICES.md (cargo about $(scripts/third-party-notices.sh --tool-version) missing): the packages lack it")
fi

step "layout in $out"
mapfile -t dirs < <(node npm/build.js --out "$out" "${bin_args[@]}" "${notices_args[@]}")
printf '%s\n' "${dirs[@]}"

step "npm pack --dry-run"
for dir in "${dirs[@]}"; do
  echo "-- ${dir#"$out"/}"
  # --dry-run: lists the tarball's contents and writes nothing; no registry is contacted.
  (cd "$dir" && npm pack --dry-run --ignore-scripts --json 2>/dev/null) |
    node npm/build.js pack-list
done

step "smoke test: the shims run the host binaries"
smoke="$out/.smoke"
rm -rf "$smoke" && mkdir -p "$smoke/node_modules"
host_pkg=$(node -p 'require("./npm/zerochat/lib/shim").platformPackage()')
mkdir -p "$(dirname "$smoke/node_modules/$host_pkg")"
cp -R "$out/zerochat" "$smoke/node_modules/"
cp -R "$out/$host_pkg" "$smoke/node_modules/$host_pkg"
node "$smoke/node_modules/zerochat/bin/zerochat.js" --version
node "$smoke/node_modules/zerochat/bin/zsf.js" --version

step "completeness check (what a release needs before npm publish)"
if node npm/build.js check "$out"; then publishable="yes (but publish from release.yml only)"; else publishable="no: see the problems above"; fi

step "summary"
version=$(node npm/build.js version)
echo "version:     $version (Cargo.toml [workspace.package])"
echo "packages:    ${#dirs[@]} laid out in $out"
for b in "${built[@]}"; do echo "built:       $b"; done
for s in "${skipped[@]}"; do echo "skipped:     $s"; done
for n in "${notes[@]}"; do echo "note:        $n"; done
echo "complete:    $publishable"
echo "published:   nothing (npm pack --dry-run only)"
