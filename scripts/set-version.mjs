// Sets the version everywhere it lives.   npm run set-version -- 0.2.0
import { readFileSync, writeFileSync } from 'node:fs';

const version = (process.argv[2] ?? '').replace(/^v/, '');
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version)) {
  console.error('Usage: npm run set-version -- 1.2.3   (or 1.2.3-beta.1)');
  process.exit(1);
}
const file = (p) => new URL(`../${p}`, import.meta.url);
const edit = (p, fn) => writeFileSync(file(p), fn(readFileSync(file(p), 'utf8')));

for (const p of ['package.json', 'src-tauri/tauri.conf.json']) edit(p, (s) => s.replace(/"version": "[^"]+"/, `"version": "${version}"`));
edit('Cargo.toml', (s) => s.replace(/(\[workspace\.package\][^[]*?version = ")[^"]+"/, `$1${version}"`));
edit('hangar.json', (s) => s.replace(/"version": "[^"]+"/, `"version": "${version}"`));
console.log(`Version set to ${version}. Add a "## ${version}" section to CHANGELOG.md, commit, then: git tag v${version} && git push origin v${version}`);
