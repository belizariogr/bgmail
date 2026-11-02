// Reference renderer: headless Chromium, offline, 800px wide, full-page PNG.
import puppeteer from 'puppeteer-core';
import { readdirSync, readFileSync, writeFileSync, unlinkSync } from 'node:fs';
import { resolve } from 'node:path';

// usage: render_chromium.mjs <corpus> <out_dir> [width=800]
const [corpus, outDir] = process.argv.slice(2, 4).map((p) => resolve(p));
const width = Number(process.argv[4] ?? 800);
const browser = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: true, args: ['--no-sandbox', '--font-render-hinting=none'] });
const page = await browser.newPage();
await page.setViewport({ width, height: 600, deviceScaleFactor: 1 });
await page.setRequestInterception(true);
page.on('request', (r) => (r.url().startsWith('file:') || r.url().startsWith('data:') ? r.continue() : r.abort()));
for (const f of readdirSync(corpus).filter((f) => f.endsWith('.html') && !f.startsWith('.')).sort()) {
  const t0 = performance.now();
  // html_view renders doctype-less mail in standards mode (as webmail does);
  // render the reference the same way through a sibling copy (same base URL).
  const html = readFileSync(`${corpus}/${f}`, 'utf8');
  const standards = /^\s*(<\?xml[^>]*>\s*)?(<!--[\s\S]*?-->\s*)*<!doctype/i.test(html);
  const src = standards ? f : `.std-${f}`;
  if (!standards) writeFileSync(`${corpus}/${src}`, '<!DOCTYPE html>\n' + html);
  await page.goto('file://' + corpus + '/' + src, { waitUntil: 'load' });
  if (!standards) unlinkSync(`${corpus}/${src}`);
  const ms = performance.now() - t0;
  await page.screenshot({ path: `${outDir}/${f.replace('.html', '.png')}`, fullPage: true, omitBackground: false });
  console.log(`${f}\t${ms.toFixed(0)}ms`);
}
await browser.close();
