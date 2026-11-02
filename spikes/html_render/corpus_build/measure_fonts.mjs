import puppeteer from 'puppeteer-core';
import { readdirSync } from 'node:fs';
const b = await puppeteer.launch({ executablePath: '/usr/bin/chromium', headless: true, args: ['--no-sandbox'] });
const p = await b.newPage(); await p.setViewport({ width: 800, height: 600 });
for (const f of readdirSync('/tmp/lh').filter(f => f.endsWith('.html')).sort()) {
  await p.goto('file:///tmp/lh/' + f);
  const r = await p.evaluate(() => { const t=document.getElementById('t'), q=document.getElementById('p'); return [t.getBoundingClientRect().height, q.getBoundingClientRect().height, getComputedStyle(t).fontFamily]; });
  // which font actually used: use CDP
  const cdp = await p.target().createCDPSession(); await cdp.send('DOM.enable'); await cdp.send('CSS.enable');
  const doc = await cdp.send('DOM.getDocument'); const n = await cdp.send('DOM.querySelector', {nodeId: doc.root.nodeId, selector: '#t'});
  const fonts = await cdp.send('CSS.getPlatformFontsForNode', {nodeId: n.nodeId});
  console.log(f, r[0].toFixed(1), r[1].toFixed(1), fonts.fonts.map(x=>x.familyName).join('|'));
}
await b.close();
