// Remove the scratch disk images `create-dmg` leaves behind when it fails.
//
// Tauri packages `src-tauri/target/release/bundle/macos/` as the DMG's source
// folder, and bundle_dmg.sh writes its temporary `rw.*.dmg` next to the app. If
// a run fails, that ~31 MB scratch image is left inside the source folder, so
// the next run copies it into the new image — which then fails too, leaving an
// even bigger one. The folder grows on every failed attempt and the DMG ends up
// bundling junk. Clearing the scratch images first breaks the loop.
import { existsSync, readdirSync, rmSync, statSync } from 'node:fs';
import { join } from 'node:path';

const dir = 'src-tauri/target/release/bundle/macos';
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