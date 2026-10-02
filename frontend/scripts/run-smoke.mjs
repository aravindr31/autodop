import { build } from 'esbuild';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const tmp = mkdtempSync('/tmp/autodop-smoke-');
const outdir = join(tmp, 'smoke-bundle');

let code = 1;
try {
  await build({ entryPoints: ['src/lib/store.ts'], outdir, bundle: true, format: 'esm', platform: 'node', logLevel: 'warning', outExtension: { '.js': '.mjs' } });
  await build({ entryPoints: ['src/lib/accounts.ts'], outdir, bundle: true, format: 'esm', platform: 'node', logLevel: 'warning', outExtension: { '.js': '.mjs' } });

  process.env.AUTODOP_SMOKE_DIR = outdir;
  const { runSmoke } = await import(new URL(`./store-smoke.mjs?t=${Date.now()}`, import.meta.url));
  code = await runSmoke();
} catch (err) {
  console.error('Smoke test error:', err instanceof Error ? err.message : err);
} finally {
  if (!process.env.AUTODOP_KEEP_SMOKE) rmSync(tmp, { recursive: true, force: true });
}
process.exit(code);
