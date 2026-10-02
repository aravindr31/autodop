#!/usr/bin/env node









import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const next = process.argv[2];

if (!next || !/^\d+\.\d+\.\d+$/.test(next)) {
  console.error('Usage: npm run version:bump -- <major.minor.patch>');
  process.exit(1);
}

const targets = [
  { file: 'src-tauri/tauri.conf.json', pattern: /("version"\s*:\s*")[^"]+(")/, name: 'tauri.conf.json (installer name)' },
  { file: 'src-tauri/Cargo.toml', pattern: /(^version\s*=\s*")[^"]+(")/m, name: 'Cargo.toml' },
  { file: 'package.json', pattern: /("version"\s*:\s*")[^"]+(")/, name: 'package.json' },
  { file: 'frontend/package.json', pattern: /("version"\s*:\s*")[^"]+(")/, name: 'frontend/package.json' },
];

for (const { file, pattern, name } of targets) {
  const path = join(root, file);
  const before = readFileSync(path, 'utf8');
  const matches = before.match(new RegExp(pattern.source, `${pattern.flags}g`)) ?? [];
  if (matches.length !== 1) {
    console.error(`${name}: expected exactly 1 version field, found ${matches.length} — not touching it`);
    process.exit(1);
  }
  const after = before.replace(pattern, `$1${next}$2`);
  if (after !== before) writeFileSync(path, after);
  console.log(`  ${name} → ${next}`);
}

console.log(`\nVersion set to ${next}. Now: npm run build:sidecar && npm run build`);
