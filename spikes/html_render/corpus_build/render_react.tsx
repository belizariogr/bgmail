// Renders the react-email demo templates to static HTML files.
import { render } from '@react-email/render';
import * as React from 'react';
import { mkdirSync, writeFileSync } from 'node:fs';

const picks: [string, string][] = JSON.parse(process.argv[2]);
const outDir = process.argv[3];
mkdirSync(outDir, { recursive: true });
for (const [file, name] of picks) {
  try {
    const mod = await import(file);
    const Comp = mod.default ?? Object.values(mod).find((v) => typeof v === 'function');
    const props = (Comp as any).PreviewProps ?? {};
    const html = await render(React.createElement(Comp as any, props));
    writeFileSync(`${outDir}/${name}.html`, html);
    console.log('ok', name, html.length);
  } catch (e) {
    console.log('FAIL', name, String(e).slice(0, 300));
  }
}
