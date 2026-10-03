#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dry = process.argv.includes('--dry');

const git = (args) =>
  execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();

const versionFile = join(root, 'VERSION');
const bumpOf = (version, part) => {
  const [major, minor, patch] = version.split('.').map(Number);
  if (part === 'major') return `${major + 1}.0.0`;
  if (part === 'minor') return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
};


const lastTag = git(['tag', '--list', 'v*', '--sort=-v:refname']).split('\n')[0] || '';
const current = lastTag ? lastTag.slice(1) : readFileSync(versionFile, 'utf8').trim();
const range = lastTag ? `${lastTag}..HEAD` : 'HEAD';

const commits = git(['log', '--format=%s%x1f%b%x1e', range])
  .split('\x1e')
  .map((r) => r.trim())
  .filter(Boolean);

if (commits.length === 0) {
  console.log(`nothing to release since ${lastTag}`);
  process.exit(0);
}

const rank = { patch: 1, minor: 2, major: 3 };
let part = null;
const raise = (p) => { if (!part || rank[p] > rank[part]) part = p; };

const groups = { feat: [], fix: [], other: [], breaking: [] };
for (const record of commits) {
  const [subject, body = ''] = record.split('\x1f');
  const breaking = /^(\w+)(\([^)]*\))?!:/.test(subject) || body.includes('BREAKING CHANGE');
  const type = /^(feat|fix|chore|docs|ci|refactor|perf|test|style|build)(\([^)]*\))?:/.exec(subject)?.[1] ?? 'other';
  if (breaking) { groups.breaking.push(subject); raise('major'); continue; }
  if (type === 'feat') { groups.feat.push(subject); raise('minor'); }
  else if (type === 'fix') { groups.fix.push(subject); raise('patch'); }
  else groups.other.push(subject);
}

const next = bumpOf(current, part ?? 'patch');


const today = new Date().toISOString().slice(0, 10);
const section = [`## v${next} — ${today}`, ''];
const push = (title, items) => {
  if (items.length === 0) return;
  section.push(`### ${title}`, '');
  for (const item of items) section.push(`- ${item.replace(/^(\w+)(\([^)]*\))?:\s*/, '')}`);
  section.push('');
};
push('Breaking changes', groups.breaking);
push('Features', groups.feat);
push('Fixes', groups.fix);
push('Other', groups.other);

const changelogPath = join(root, 'CHANGELOG.md');
let changelog = readFileSync(changelogPath, 'utf8');

const heading = `## v${next}`;
const start = changelog.indexOf(`\n${heading}`);
if (start >= 0) {
  const nextStart = changelog.indexOf('\n## v', start + 1);
  changelog = changelog.slice(0, start) + '\n' + section.join('\n') + (nextStart >= 0 ? changelog.slice(nextStart) : '');
} else {
  const bodyStart = changelog.indexOf('\n', changelog.indexOf('# Changelog')) + 1;
  changelog = `${changelog.slice(0, bodyStart)}\n${section.join('\n')}${changelog.slice(bodyStart)}`;
}

console.log(`last release: ${lastTag || '(none)'} · commits considered: ${commits.length}`);
console.log(`bump: ${part ?? 'patch (default)'} → ${current} becomes ${next}`);

if (dry) {
  console.log(section.join('\n'));
  process.exit(0);
}
if (dry) {
  console.log(section.join('\n'));
  process.exit(0);
}

execFileSync('node', [join(root, 'scripts/bump-version.mjs'), next], {
  cwd: root,
  stdio: 'inherit',
});

writeFileSync(versionFile, `${next}\n`);
writeFileSync(changelogPath, changelog);
console.log(`\nVERSION → ${next}; CHANGELOG.md updated.`);
console.log(`\nVERSION → ${next}; CHANGELOG.md updated.`);
