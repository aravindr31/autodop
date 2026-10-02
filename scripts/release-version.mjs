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
const current = readFileSync(versionFile, 'utf8').trim();
const bumpOf = (version, part) => {
  const [major, minor, patch] = version.split('.').map(Number);
  if (part === 'major') return `${major + 1}.0.0`;
  if (part === 'minor') return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
};


const lastTag = git(['tag', '--list', 'v*', '--sort=-v:refname']).split('\n')[0] || '';
const range = lastTag ? `${lastTag}..HEAD` : 'HEAD';
const commits = git(['log', '--format=%s%x09%b', range]).split('\n').filter(Boolean);

let part = null;
const groups = { feat: [], fix: [], other: [], breaking: [] };
for (const line of commits) {
  const [subject, ...body] = line.split('\t');
  const breaking = /^(\w+)(\([^)]*\))?!:/.test(subject) || body.join('').includes('BREAKING CHANGE');
  const type = /^(feat|fix|chore|docs|ci|refactor|perf|test|style|build)(\([^)]*\))?:/.exec(subject)?.[1] ?? 'other';
  if (breaking) {
    groups.breaking.push(subject);
    part = 'major';
    continue;
  }
  if (type === 'feat') {
    groups.feat.push(subject);
    part ??= 'minor';
  } else if (type === 'fix') {
    groups.fix.push(subject);
    part ??= 'patch';
  } else {
    groups.other.push(subject);
  }
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

writeFileSync(versionFile, `${next}\n`);
writeFileSync(changelogPath, changelog);


const targets = [
  { file: 'src-tauri/tauri.conf.json', pattern: /("version"\s*:\s*")[^"]+(")/ },
  { file: 'src-tauri/Cargo.toml', pattern: /(^version\s*=\s*")[^"]+(")/m },
  { file: 'package.json', pattern: /("version"\s*:\s*")[^"]+(")/ },
  { file: 'frontend/package.json', pattern: /("version"\s*:\s*")[^"]+(")/ },
];
for (const { file, pattern } of targets) {
  const path = join(root, file);
  const before = readFileSync(path, 'utf8');
  writeFileSync(path, before.replace(new RegExp(pattern.source, `${pattern.flags}g`), `$1${next}$2`));
  console.log(`  ${file} → ${next}`);
}
console.log(`\nVERSION → ${next}; CHANGELOG.md updated.`);
