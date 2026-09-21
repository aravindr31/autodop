// Remove the scratch disk images `create-dmg` leaves behind when it fails, and
// detach any staging volume a failed run left mounted.
//
// Tauri packages `src-tauri/target/release/bundle/macos/` as the DMG's source
// folder, and the scratch `rw.*.dmg` ends up next to the app. If a run fails,
// that ~31 MB image is left inside the source folder, so the next run copies it
// into the new image — which then fails too, leaving an even bigger one. The
// folder grows on every failed attempt and the DMG ends up bundling junk.
//
// A failed run can also leave its staging volume attached, which makes the very
// next run fail. Clearing both first breaks the loop.
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';

const dir = 'src-tauri/target/release/bundle/macos';
const bundleDir = resolve('src-tauri/target/release/bundle');

/**
 * Detach volumes left mounted by a failed run.
 *
 * Only volumes backed by an image inside this repo's own bundle directory are
 * touched, so an unrelated disk image the user has mounted is never disturbed.
 */
function detachStaleStagingVolumes() {
  let info;
  try {
    info = execFileSync('hdiutil', ['info'], { encoding: 'utf8' });
  } catch {
    return; // hdiutil unavailable — nothing to do
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