// Draws Marquee's icon (a marquee sign: a dark tile, a border of bulbs, a gold M) and renders it to
// src-tauri/icons/app-icon.png. `npm run icons` then has the Tauri CLI cut every Windows size from it.
// The drawing is the same one the app shows in its sidebar (src/ui/Logo.tsx).
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const GOLD = '#F2C14E';
const bulbs = [];
const n = 5;
for (let i = 0; i < n; i++) bulbs.push([14 + (36 / (n - 1)) * i, 9]);
for (let i = 1; i < n - 1; i++) bulbs.push([55, 9 + (46 / (n - 1)) * i]);
for (let i = n - 1; i >= 0; i--) bulbs.push([14 + (36 / (n - 1)) * i, 55]);
for (let i = n - 2; i > 0; i--) bulbs.push([9, 9 + (46 / (n - 1)) * i]);

const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="1024" height="1024">
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#17171B"/><stop offset="1" stop-color="#0A0A0C"/></linearGradient>
    <radialGradient id="glow" cx="50%" cy="50%" r="50%"><stop offset="0" stop-color="${GOLD}" stop-opacity="0.22"/><stop offset="1" stop-color="${GOLD}" stop-opacity="0"/></radialGradient>
  </defs>
  <rect x="3" y="3" width="58" height="58" rx="15" fill="url(#tile)"/>
  <rect x="3" y="3" width="58" height="58" rx="15" fill="url(#glow)"/>
  <rect x="3.75" y="3.75" width="56.5" height="56.5" rx="14.25" fill="none" stroke="${GOLD}" stroke-opacity="0.4" stroke-width="1.5"/>
  ${bulbs.map(([x, y], i) => `<circle cx="${x}" cy="${y}" r="2.3" fill="${GOLD}" fill-opacity="${i % 2 ? 0.55 : 1}"/>`).join('\n  ')}
  <path d="M19.5 45V20.5h4.7L32 34l7.8-13.5h4.7V45h-4.8V30.200L33.600 41h-3.200l-6.100-10.800V45z" fill="${GOLD}"/>
</svg>
`;

const out = (name) => fileURLToPath(new URL(`../src-tauri/icons/${name}`, import.meta.url));
writeFileSync(out('app-icon.svg'), svg);

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL || undefined });
const page = await browser.newPage({ viewport: { width: 1024, height: 1024 }, deviceScaleFactor: 1 });
await page.setContent(`<style>html,body{margin:0;background:transparent}svg{display:block}</style>${svg}`);
await page.screenshot({ path: out('app-icon.png'), omitBackground: true });
// The tray uses a simpler mark that stays legible at 16 px: the tile and the M, no bulbs.
const tray = svg.replace(/<circle[^>]*\/>\s*/g, '').replace('stroke-opacity="0.4" stroke-width="1.5"', 'stroke-opacity="0.9" stroke-width="2.5"');
await page.setViewportSize({ width: 64, height: 64 });
await page.setContent(`<style>html,body{margin:0;background:transparent}svg{display:block;width:64px;height:64px}</style>${tray}`);
await page.screenshot({ path: out('tray.png'), omitBackground: true });
await browser.close();
console.log('Wrote app-icon.svg, app-icon.png and tray.png');
