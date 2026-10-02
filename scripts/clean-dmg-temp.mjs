import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';

const dir = 'src-tauri/target/release/bundle/macos';
const bundleDir = resolve('src-tauri/target/release/bundle');







function detachStaleStagingVolumes() {
  let info;
  try {
    info = execFileSync('hdiutil', ['info'], { encoding: 'utf8' });
  } catch {
    return;
  }

  let backedByOurImage = false;
  const mounted = new Set();
  for (const line of info.split('\n')) {
    const image = line.match(/^image-path\s+:\s+(.+)$/);
    if (image) {
      backedByOurImage = resolve(image[1].trim()).startsWith(bundleDir);
      continue;
    }
    const volume = line.match(/(\/Volumes\/[^\t]+)\s*$/);
    if (backedByOurImage && volume) {
      mounted.add(volume[1].trim());
    }
  }

  for (const volume of mounted) {
    try {
      execFileSync('hdiutil', ['detach', volume], { stdio: 'ignore' });
      console.log(`detached stale staging volume ${volume}`);
    } catch {
      console.log(`could not detach ${volume} — skipping`);
    }
  }
}

detachStaleStagingVolumes();

if (!existsSync(dir)) {
  process.exit(0);
}

let removed = 0;
for (const entry of readdirSync(dir)) {
  if (!entry.startsWith('rw.') || !entry.endsWith('.dmg')) continue;
  const path = join(dir, entry);
  const megabytes = (statSync(path).size / 1024 / 1024).toFixed(1);
  rmSync(path);
  console.log(`removed leftover DMG scratch image ${entry} (${megabytes} MB)`);
  removed += 1;
}

console.log(removed === 0 ? 'no leftover DMG scratch images' : `removed ${removed}`);
