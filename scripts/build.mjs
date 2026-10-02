#!/usr/bin/env node
/**
 * Build the app and package it into a .dmg — reliably.
 *
 * Tauri's own DMG step (create-dmg) writes its scratch `rw.*.dmg` image into the
 * folder it is about to copy, so the image ends up containing a copy of itself:
 * a failed run leaves tens of MB behind and every retry nests deeper, ending in
 * a confusing "No space left on device" from `hdiutil resize`. That has bitten
 * this project repeatedly.
 *
 * So: build the `.app` with Tauri, stage a pristine copy *outside* the bundle
 * tree, and make the image with one `hdiutil create`. Then mount it read-only
 * and check what actually landed inside before declaring success.
 *
 *   npm run build
 */
import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const tauri = existsSync(join(root, 'node_modules', '.bin', 'tauri'))
  ? join(root, 'node_modules', '.bin', 'tauri')
  : 'npx';
// Windows shells resolve `.bin/tauri` to the `.cmd` shim; execFileSync needs
// the exact file.
const tauriCmd = process.platform === 'win32' && existsSync(`${tauri}.cmd`) ? `${tauri}.cmd` : tauri;

const run = (cmd, args, options = {}) =>
  execFileSync(cmd, args, { stdio: 'inherit', cwd: root, ...options });

// Bundle types differ by platform: `app` is the macOS-only bundle the DMG
// step below builds on; Windows and Linux let Tauri pick (NSIS/MSI, AppImage,
// deb) — passing `--bundles app` there would fail outright.
const isMac = process.platform === 'darwin';
run(
  tauriCmd,
  tauriCmd === 'npx'
    ? ['tauri', 'build', ...(isMac ? ['--bundles', 'app'] : [])]
    : ['build', ...(isMac ? ['--bundles', 'app'] : [])],
);

const config = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const product = config.productName;
const version = config.version;

// On other platforms Tauri's own bundler is fine — .msi/.exe (NSIS) on
// Windows, .AppImage/.deb on Linux.
if (!isMac) {
  const dir = join(root, 'src-tauri', 'target', 'release', 'bundle');
  console.log(`\n${process.platform}: Tauri packaging complete — see ${dir}`);
  process.exit(0);
}

const srcApp = join(root, 'src-tauri', 'target', 'release', 'bundle', 'macos', `${product}.app`);

if (!existsSync(srcApp)) {
  console.error(`No app bundle at ${srcApp}`);
  process.exit(1);
}

// 2. stage a clean copy well away from any bundle directory
const stage = mkdtempSync(join(tmpdir(), 'autodop-dmg-'));
const stagedApp = join(stage, `${product}.app`);
cpSync(srcApp, stagedApp, { recursive: true, verbatimSymlinks: true });
// The familiar drag-to-install target.
symlinkSync('/Applications', join(stage, 'Applications'));

const dmgPath = join(root, 'src-tauri', 'target', 'release', 'bundle', 'dmg', `${product}_${version}_${process.arch === 'arm64' ? 'aarch64' : 'x64'}.dmg`);
mkdirSync(dirname(dmgPath), { recursive: true });

// 3. one shot, no scratch image anywhere near the source
run('hdiutil', [
  'create',
  '-volname', product,
  '-srcfolder', stage,
  '-ov',
  '-format', 'UDZO',
  dmgPath,
]);

// 4. never claim success without looking inside
const mountPoint = mkdtempSync(join(tmpdir(), 'autodop-mnt-'));
try {
  run('hdiutil', ['attach', '-nobrowse', '-readonly', '-mountpoint', mountPoint, dmgPath], {
    stdio: 'pipe',
  });
  const inside = join(mountPoint, `${product}.app`, 'Contents');
  const checks = [
    ['the app', existsSync(join(mountPoint, `${product}.app`))],
    ['the installer symlink', existsSync(join(mountPoint, 'Applications'))],
    ['the bundled .py', existsSync(join(inside, 'Resources', '_up_', 'scraper.py'))],
    ['the frozen runner', existsSync(join(inside, 'Resources', 'binaries', `scraper-macos-${process.arch === 'arm64' ? 'arm64' : 'x64'}`))],
  ];
  let bad = 0;
  for (const [label, ok] of checks) {
    if (!ok) bad += 1;
    console.log(`  ${ok ? 'OK  ' : 'MISS'}  ${label}`);
  }

  const { size } = statSync(dmgPath);
  console.log(`\n${dmgPath}`);
  console.log(`  version ${version}, ${(size / 1024 / 1024).toFixed(1)} MB`);
  if (bad > 0) {
    console.error(`\n${bad} check(s) failed — do not ship this.`);
    process.exitCode = 1;
  }
} finally {
  try {
    execFileSync('hdiutil', ['detach', mountPoint], { stdio: 'pipe' });
  } catch {
    /* already detached */
  }
  rmSync(stage, { recursive: true, force: true });
  rmSync(mountPoint, { recursive: true, force: true });
}
