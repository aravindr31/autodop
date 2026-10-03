#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const tauri = existsSync(join(root, 'node_modules', '.bin', 'tauri'))
  ? join(root, 'node_modules', '.bin', 'tauri')
  : 'npx';


const tauriCmd = process.platform === 'win32' && existsSync(`${tauri}.cmd`) ? `${tauri}.cmd` : tauri;

const run = (cmd, args, options = {}) =>

  execFileSync(cmd, args, { stdio: 'inherit', cwd: root, shell: process.platform === 'win32', ...options });

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

// Ad-hoc code signature. Without any signature, a *downloaded* DMG is
// quarantined and Gatekeeper reports the app as "damaged" rather than merely
// "unidentified developer" — ad-hoc signing moves it back to the fixable case
// (right-click → Open) without needing a Developer ID certificate.
try {
  run('codesign', ['--force', '--deep', '--sign', '-', srcApp]);
} catch (error) {
  console.warn('ad-hoc codesign failed — continuing; users may need xattr -cr', error);
}


const stage = mkdtempSync(join(tmpdir(), 'autodop-dmg-'));
const stagedApp = join(stage, `${product}.app`);
cpSync(srcApp, stagedApp, { recursive: true, verbatimSymlinks: true });

symlinkSync('/Applications', join(stage, 'Applications'));

const dmgPath = join(root, 'src-tauri', 'target', 'release', 'bundle', 'dmg', `${product}_${version}_${process.arch === 'arm64' ? 'aarch64' : 'x64'}.dmg`);
mkdirSync(dirname(dmgPath), { recursive: true });


run('hdiutil', [
  'create',
  '-volname', product,
  '-srcfolder', stage,
  '-ov',
  '-format', 'UDZO',
  dmgPath,
]);


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

  }
  rmSync(stage, { recursive: true, force: true });
  rmSync(mountPoint, { recursive: true, force: true });
}
