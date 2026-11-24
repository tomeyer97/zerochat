# Verify a download

Every release is built by [release.yml](../.github/workflows/release.yml) from a signed `vX.Y.Z` tag of
https://github.com/tomeyer97/zerochat. These checks prove that what you got is what that workflow built.
You need `sha256sum` (macOS: `shasum -a 256`) and, for attestations, the [GitHub CLI](https://cli.github.com/).

## What a release holds

| File | What |
|---|---|
| `zerochat-<target>.tar.xz`, `zsf-<target>.tar.xz` | the binary, `LICENSE`, `README.md`, `THIRD_PARTY_NOTICES.md` |
| `<archive>.sha256` | the archive's sha256 |
| `sha256.sum` | all checksums in one file |
| `zerochat-installer.sh`, `zsf-installer.sh` | the shell installers |
| `*.cdx.json` | CycloneDX SBOMs: the Rust crates per target, and `zerochat-web.cdx.json` (the web UI) |
| `THIRD_PARTY_NOTICES.md` | the licences of everything compiled in |

Targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`,
`aarch64-apple-darwin`.

## 1. Checksums

```sh
V=vX.Y.Z
gh release download "$V" --repo tomeyer97/zerochat --pattern 'zerochat-x86_64-unknown-linux-musl.tar.xz*'
sha256sum -c zerochat-x86_64-unknown-linux-musl.tar.xz.sha256
# or, with sha256.sum downloaded too:
sha256sum -c --ignore-missing sha256.sum
```

A checksum only proves the download is intact. The attestation proves where it came from.

## 2. Attestations (provenance)

The archives and the SBOMs carry GitHub build-provenance attestations, signed for this repository's
release workflow:

```sh
gh attestation verify zerochat-x86_64-unknown-linux-musl.tar.xz --repo tomeyer97/zerochat
```

Look for `✓ Verification succeeded!` and check the tag in the output is the version you wanted.
Every attestation: https://github.com/tomeyer97/zerochat/attestations.

## 3. SBOM

```sh
gh release download "$V" --repo tomeyer97/zerochat --pattern '*.cdx.json'
gh attestation verify zerochat-web.cdx.json --repo tomeyer97/zerochat
```

Feed the SBOMs to your scanner (e.g. `grype sbom:zerochat-web.cdx.json`) to check the dependencies.

## 4. The shell installer

The installer is `curl | sh` friendly, but read it before you run it:

```sh
gh release download "$V" --repo tomeyer97/zerochat --pattern zerochat-installer.sh
less zerochat-installer.sh        # a plain sh script
grep -n 'releases/download' zerochat-installer.sh   # where it downloads from: this release
sh zerochat-installer.sh
```

To skip the installer: download the archive, verify it (steps 1 and 2), and unpack the binary onto your
`PATH`. Install options: [install.md](install.md).

## 5. npm

The npm packages (`zerochat` and `@zerochat/cli-<os>-<arch>`) are laid out from the same attested
archives and published by the same workflow with npm provenance.

```sh
npm i -g zerochat
npm audit signatures              # checks registry signatures and provenance attestations
zerochat --version
```

- On https://www.npmjs.com/package/zerochat, the version page shows **Provenance**: it must link to
  `tomeyer97/zerochat`, the workflow `.github/workflows/release.yml` and the tag `vX.Y.Z`.
- The same holds for each `@zerochat/cli-*` package.

## Something does not match

Do not run it. Report it privately as described in [SECURITY.md](../SECURITY.md).
