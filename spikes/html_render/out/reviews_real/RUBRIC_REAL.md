# Review brief: html_view vs Chromium on REAL e-mails

BGMail renders HTML e-mail natively (Blitz layout painted by GPUI). This round
uses 45 real messages: `ham-*` are legitimate 2002-era mails (old HTML:
font tags, nested tables, spacer GIFs), `rge-*` are 2025 brand campaigns.
Both engines render offline: every remote image is blocked in BOTH columns.

Sheets: `out_real_hv6_w800/sheets/<email>__<N>.png` (N = vertical segments;
columns scaled). LEFT = Chromium (reference), MIDDLE = html_view (under
test), RIGHT = duplicate of html_view (ignore). View EVERY segment of each
assigned e-mail. Full-resolution renders: `out/real_chromium_w800_vp/<email>.png`
and `out/real_html_view_it6_w800/<email>.png`. `out_real_hv6_w800/scores.tsv`
has heights and SSIM (hint only).

## Ignore (not defects)
- Font family / glyph shape / antialiasing; 1–3 px drifts; < ~5% height.
- Chromium's broken-image ICON (small torn-picture glyph). html_view shows
  the image's alt text / a silver frame instead; judge only whether the
  alt text/space is present and laid out like Chromium's.
- Remote images missing (blocked in both).

## Score html_view (MIDDLE) 0–10 against Chromium
- 10 identical apart from ignored items; 9–9.9 minor deviations a user
  wouldn't notice; 8–8.9 small visible deviations; 6–7 noticeable defects
  but readable; 4–5 significant layout defects (broken columns, missing
  content, wrong widths); 0–3 badly broken/unusable.

## Issues
Concrete problems in **Brazilian Portuguese**, each with a severity tag
(`[crítico]`, `[alto]`, `[médio]`, `[baixo]`) and, when you can tell, the
CSS/HTML feature responsible (`css_suspect`). Be specific (element, what
differs, where, how much).

## Output
Write the JSON file given in your task: an object mapping "<email>" to
`{"score": <number>, "issues": [...], "css_suspect": "..."}`; validate it
with `python3 -m json.tool`. Final reply: path, average, every score < 9
with its main issue, and the 3 most frequent root causes you saw.
