'use strict';
// The addon this platform runs (crates/delurio-node, built with napi-rs): in a checkout, a local build at the
// package's root (`npm run build`: delurio.<target>.node); else the prebuilt one of @delurio/cli-<target>, the
// optional dependency npm installs only on the platform it is for. DELURIO_ADDON names another file to load.
// Under Node's permission model a native addon needs --allow-addons.
const { existsSync } = require('node:fs');
const { join } = require('node:path');

// glibc says its version in the process report; musl (Alpine) says none
function libc() {
  const report = typeof process.report?.getReport === 'function' ? process.report.getReport() : null;
  const header = report !== null && typeof report === 'object' ? report.header : undefined;
  return header !== undefined && header.glibcVersionRuntime !== undefined ? 'gnu' : 'musl';
}

function target() {
  const { platform, arch } = process;
  if (platform === 'linux') return `linux-${arch}-${libc()}`;
  if (platform === 'win32') return `win32-${arch}-msvc`;
  return `${platform}-${arch}`;
}

function load() {
  const t = target();
  const tried = [];
  const candidates = [];
  if (process.env.DELURIO_ADDON) candidates.push(process.env.DELURIO_ADDON);
  candidates.push(join(__dirname, '..', `delurio.${t}.node`));
  for (const file of candidates) {
    if (!existsSync(file)) {
      tried.push(`${file}: not there`);
      continue;
    }
    return require(file);
  }
  try {
    return require(`@delurio/cli-${t}`);
  } catch (err) {
    tried.push(`@delurio/cli-${t}: ${err instanceof Error ? err.message.split('\n')[0] : String(err)}`);
  }
  throw new Error(
    `@delurio/cli has no build for ${t} here. npm installs the one for this platform (@delurio/cli-${t}) as an optional dependency: install without --no-optional / --omit=optional, and under Node's permission model allow it with --allow-addons. Tried:\n  ${tried.join('\n  ')}`,
  );
}

module.exports = load();
