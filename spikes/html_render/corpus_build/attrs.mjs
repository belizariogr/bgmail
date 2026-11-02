import puppeteer from 'puppeteer-core';
import { resolve } from 'node:path';
const [file, ...idx] = process.argv.slice(2);
const b = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: true, args: ['--no-sandbox'] });
const p = await b.newPage(); await p.setViewport({ width: 800, height: 600 });
await p.setRequestInterception(true);
p.on('request', (r) => (r.url().startsWith('file:') || r.url().startsWith('data:') ? r.continue() : r.abort()));
await p.goto('file://' + resolve(file));
const out = await p.evaluate((idx) => {
  const skip = new Set(['SCRIPT','STYLE','HEAD','TITLE','META','LINK','TBODY','THEAD','TFOOT','TR']);
  const list = []; const walk = (el) => { if (!skip.has(el.tagName)) list.push(el); for (const c of el.children) walk(c); }; walk(document.body);
  return idx.map(i => { const el = list[+i]; const cs = getComputedStyle(el); const o = el.outerHTML.replace(/src="[^"]*"/g,'src=".."');
    return `#${i} ${el.tagName} margin=${cs.margin} padding=${cs.padding} display=${cs.display} lh=${cs.lineHeight} fs=${cs.fontSize} va=${cs.verticalAlign}\n   ${o.slice(0, 400)}`; });
}, idx);
console.log(out.join('\n'));
await b.close();
