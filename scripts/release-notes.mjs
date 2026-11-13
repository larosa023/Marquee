// Prints one version's section of CHANGELOG.md, for the GitHub Release body (which is also what
// the in-app "What's new" dialog shows).   node scripts/release-notes.mjs 0.1.0
import { readFileSync } from 'node:fs';

const version = (process.argv[2] ?? '').replace(/^v/, '');
const base = version.split('-')[0];
const log = readFileSync(new URL('../CHANGELOG.md', import.meta.url), 'utf8');
const sections = log.split(/^## /m).slice(1);
const section = sections.find((s) => s.startsWith(version)) ?? sections.find((s) => s.startsWith(base));
if (!section) {
  console.error(`CHANGELOG.md has no section for ${version}.`);
  process.exit(1);
}
process.stdout.write(section.replace(/^[^\n]*\n/, '').trim() + '\n');
