'use strict';
// Runs the native zerochat or zsf binary from the platform package npm installed as an optional
// dependency (no postinstall: `--ignore-scripts` and pnpm work). Error wording follows Zeroshot's
// bin/zeroshot.js, so users meet familiar messages.

const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const SPEC = require('../packages.json');

/** The platform package for this host, e.g. `@zerochat/cli-linux-x64` (packages.json). */
function platformPackage(platform = process.platform, arch = process.arch, spec = SPEC) {
  const found = spec.targets.find((t) => t.platform === platform && t.arch === arch);
  if (!found) {
    const hosts = spec.targets.map((t) => `${t.platform}/${t.arch}`).join(', ');
    const hint = platform === 'win32' ? '; on Windows, use WSL' : '';
    throw new Error(`UNSUPPORTED_ZEROCHAT_HOST: no prebuilt binary for ${platform}/${arch}; supported hosts: ${hosts}${hint}`);
  }
  return `${spec.platformPrefix}${platform}-${arch}`;
}

/**
 * The path of `program` (zerochat or zsf) in the platform package. `resolve` finds a package's
 * package.json (require.resolve by default, so a sibling or nested node_modules both work).
 */
function binaryPath(program, options = {}) {
  const { platform = process.platform, arch = process.arch, spec = SPEC } = options;
  const resolve = options.resolve || ((request) => require.resolve(request));
  const pkg = platformPackage(platform, arch, spec);
  let manifest;
  try {
    manifest = resolve(`${pkg}/package.json`);
  } catch {
    throw new Error(
      `NATIVE_BINARY_MISSING: ${pkg} is not installed; reinstall ${spec.name} with optional dependencies enabled (npm i -g ${spec.name}, without --omit=optional or --no-optional)`
    );
  }
  const binary = path.join(path.dirname(manifest), 'bin', program + (platform === 'win32' ? '.exe' : ''));
  if (!fs.existsSync(binary)) {
    throw new Error(`NATIVE_BINARY_MISSING: ${binary}; reinstall ${spec.name} (npm i -g ${spec.name})`);
  }
  return binary;
}

const FORWARDED = ['SIGINT', 'SIGTERM', 'SIGHUP'];

/** Runs `program` with this process's arguments; its exit code or signal becomes ours. */
function main(program) {
  let binary;
  try {
    binary = binaryPath(program);
  } catch (error) {
    process.stderr.write(`${program}: ${error.message}\n`);
    process.exitCode = 1;
    return;
  }
  // spawn, not spawnSync: a blocked event loop could not pass a SIGTERM sent to this shim on to
  // the server, which would then outlive it.
  const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit' });
  const forward = (signal) => child.kill(signal);
  for (const signal of FORWARDED) process.on(signal, forward);
  child.on('error', (error) => {
    process.stderr.write(`${program}: ${error.message}\n`);
    process.exitCode = 1;
  });
  child.on('exit', (code, signal) => {
    for (const s of FORWARDED) process.off(s, forward);
    if (signal) process.kill(process.pid, signal);
    else process.exitCode = code === null ? 1 : code;
  });
}

module.exports = { platformPackage, binaryPath, main };
