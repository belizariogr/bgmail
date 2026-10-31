# Review brief: html_view fidelity vs Chromium at several viewport widths

BGMail renders HTML e-mail natively (Blitz layout painted by GPUI) in a reader
pane whose width varies. Each e-mail was rendered by **Chromium** (reference)
and **html_view** (engine under test) at one of 375, 600, 800 or 1280 px.
Mail wider than the viewport overflows to the right in both engines; both
images show only the first <width> px (what a reader sees before scrolling
horizontally), so cut-off content at the right edge is NOT a defect if it is
cut in both.

Review sheets: `out_hv17_w<W>/sheets/<email>__<N>.png` (N = 0,1,2… vertical
segments, columns scaled). Columns: LEFT = Chromium, MIDDLE = html_view,
RIGHT = a duplicate of html_view (ignore it). View EVERY segment of each
assigned e-mail. Full-resolution renders: `out/chromium_w<W>_vp/<email>.png`
(Chromium, cropped to the viewport) and `out/html_view_it17_w<W>/<email>.png`.
`out_hv17_w<W>/scores.tsv` has heights and a coarse SSIM: hint only.

## Ignore (not defects)
- Font family / glyph shape / antialiasing differences, unless they cause a
  layout problem (overflow, broken wrapping, wrong hierarchy).
- Small (< ~5%) total-height differences from line-height/font metrics and
  1–3 px vertical drifts.
- The recast.ai logo in `mjml-recast` is missing for both (dead URL).
- The coloured header strips at the top of each column.

## Score html_view (MIDDLE column) 0–10 against Chromium
- 10: indistinguishable apart from ignored differences.
- 9–9.9: minor deviations a user would not notice.
- 8–8.9: small but visible spacing/alignment deviations.
- 6–7: noticeable defects (misalignment, wrong widths, missing radius/shadow/
  gradient), fully readable.
- 4–5: significant layout defects (broken columns, missing images, wrong
  container width, columns not stacking on mobile).
- 0–3: badly broken / unusable.

## Issues
List concrete problems in **Brazilian Portuguese**, each prefixed with a
severity tag (`[crítico]`, `[alto]`, `[médio]`, `[baixo]`), e.g.
"[médio] Botão 'Shop now' 20px mais alto que no Chromium". Be specific
(element, what differs, where, roughly how much). Add a `css_suspect` guess.

## Output
Write the JSON file given in your task: an object mapping "<email>@<W>" to
`{"score": <number>, "issues": [...], "css_suspect": "..."}`. Validate it
(`python3 -m json.tool`). Final reply: path + average score + any score < 9.5
with its main issue.
