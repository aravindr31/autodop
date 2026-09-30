#!/usr/bin/env node
/**
 * Freeze `scraper.py` into one executable and put it where Tauri bundles it, so
 * a shipped app needs no Python, no pip and no selenium on the target machine.
 *
 *   npm run build:sidecar
 *
 * PyInstaller cannot cross-compile: run this on each OS you ship, on the
 * architecture you are targeting. The filename mirrors `sidecar_name()` in
 * src-tauri/src/lib.rs — keep the two in step.
 */
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, chmodSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

const OS = { darwin: 'macos', win32: 'windows', linux: 'linux' }[process.platform] ?? process.platform;
const ARCH = { arm64: 'arm64', x64: 'x64', arm: 'arm' }[process.arch] ?? process.arch;
const suffix = process.platform === 'win32' ? '.exe' : '';
const name = `scraper-${OS}-${ARCH}`;

const binariesDir = join(root, 'src-tauri', 'binaries');
const output = join(binariesDir, `${name}${suffix}`);
const distDir = join(root, '.sidecar-build', 'dist');
const workDir = join(root, '.sidecar-build', 'work');

/** Interpreters to try, most specific first. */
function interpreters() {
  return [
    process.env.AUTODOP_PYTHON,
    join(root, '.venv-scraper', 'bin', 'python'),
    join(root, '.venv-scraper', 'Scripts', 'python.exe'),
    process.platform === 'win32' ? 'python' : 'python3',
  ].filter(Boolean);
}

const python = interpreters().find((candidate) => {
  try {
    execFileSync(candidate, ['-c', 'import sys'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
});

if (!python) {
  console.error(
    'No Python found. Set AUTODOP_PYTHON, or create .venv-scraper (see README → Python & Chrome).',
  );
  process.exit(1);
}

console.log(`Freezing scraper.py for ${OS}-${ARCH} with ${python}`);

// selenium pulls in submodules lazily; collect them explicitly or the frozen
// binary fails at import time.
try {
  execFileSync(python, ['-m', 'PyInstaller', '--version'], { stdio: 'ignore' });
} catch {
  console.log('Installing PyInstaller…');
  execFileSync(python, ['-m', 'pip', 'install', '-q', '--upgrade', 'pyinstaller'], {
    stdio: 'inherit',
  });
}

execFileSync(
  python,
  [
    '-m', 'PyInstaller',
    '--onefile',
    '--noconfirm',
    '--clean',
    '--name', name,
    '--distpath', distDir,
    '--workpath', workDir,
    '--specpath', workDir,
    '--collect-submodules', 'selenium',
    '--collect-submodules', 'webdriver_manager',
    join(root, 'scraper.py'),
  ],
  { stdio: 'inherit', cwd: root },
);

const built = join(distDir, `${name}${suffix}`);
if (!existsSync(built)) {
  console.error(`PyInstaller did not produce ${built}`);
  process.exit(1);
}

mkdirSync(binariesDir, { recursive: true });
copyFileSync(built, output);
if (process.platform !== 'win32') chmodSync(output, 0o755);

const size = (statSync(output).size / 1024 / 1024).toFixed(1);
console.log(`\nWrote ${output} (${size} MB)`);
console.log('Now: npm run build   — the app will prefer this over the bundled .py');
