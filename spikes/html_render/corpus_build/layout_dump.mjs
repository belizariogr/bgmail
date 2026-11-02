// Dumps every element's border box (document order) from Chromium as TSV.
import puppeteer from 'puppeteer-core';
import { resolve, dirname, basename } from 'node:path';
import { readFileSync, writeFileSync, unlinkSync } from 'node:fs';
const [file, width = '800'] = process.argv.slice(2);
const b = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: true, args: ['--no-sandbox'] });
const p = await b.newPage();
await p.setViewport({ width: +width, height: 600 });
await p.setRequestInterception(true);
p.on('request', (r) => (r.url().startsWith('file:') || r.url().startsWith('data:') ? r.continue() : r.abort()));
// Doctype-less mail is compared in standards mode, like the screenshots
// (render_chromium.mjs): load a sibling copy with an HTML5 doctype.
const path = resolve(file);
const html = readFileSync(path, 'utf8');
const standards = /^\s*(<\?xml[^>]*>\s*)?(<!--[\s\S]*?-->\s*)*<!doctype/i.test(html);
const src = standards ? path : `${dirname(path)}/.std-${process.pid}-${basename(path)}`;
if (!standards) writeFileSync(src, '<!DOCTYPE html>\n' + html);
await p.goto('file://' + src, { waitUntil: 'load' });
if (!standards) unlinkSync(src);
const rows = await p.evaluate(() => {
  const out = [];
  const skip = new Set(['SCRIPT', 'STYLE', 'HEAD', 'TITLE', 'META', 'LINK']);
  const walk = (el) => {
    if (!skip.has(el.tagName)) {
      const r = el.getBoundingClientRect();
      out.push([el.tagName.toLowerCase(), r.x, r.y + window.scrollY, r.width, r.height].map((v, i) => (i ? Math.round(v) : v)).join('\t'));
    }
    for (const c of el.children) walk(c);
  };
  walk(document.body);
  return out;
});
console.log(rows.join('\n'));
await b.close();
