# Review brief: HTML e-mail render fidelity (Blitz and litehtml vs Chromium)

Context: we are choosing an HTML/CSS engine to render e-mails natively inside a
Rust desktop mail client. Each e-mail was rendered at 800px viewport width by:
- **Chromium** (reference, left column, dark-grey header strip)
- **html_view** (middle column, ORANGE header strip): the engine under test —
  Blitz layout painted natively by GPUI, text shaped by GPUI.
- **Blitz CPU reference** (right column, BLUE header strip): same Blitz layout
  rasterised on the CPU. NOT scored; use it to classify each html_view issue as
  `layout` (also wrong in the right column) or `paint` (only wrong in the
  middle column: GPUI drawing/text problem).

Review sheets: `out_hv/sheets/<email>__<N>.png` (the folder is given in your task) (N = 0,1,2… vertical segments of
the same e-mail, 1100px tall each, columns scaled to 520px). View EVERY segment
of every e-mail in your batch. If a detail is unclear, you may also open the
full-resolution renders at the full-resolution renders in the `chromium/`, `blitz/` (= html_view) folders of that same sheets parent dir.
Metrics (heights + a coarse SSIM vs Chromium) are in `out/scores.tsv`: use only
as a hint; your visual judgement decides.

## What to IGNORE (not defects)
- Font family / glyph shape / antialiasing differences: the engines use their
  own system font fallback (remote web fonts were removed for all three).
  Only count fonts if they cause a layout problem (overflow, wrapping that
  breaks a layout, wrong weight/size that changes hierarchy).
- Small (< ~5%) differences in total height caused by line-height/font metrics.
- The recast.ai logo in `mjml-recast` is missing for all engines (dead URL).
- The coloured header strips at the top of each column.

## Score html_view (middle column) 0–10 against Chromium
- 10: indistinguishable apart from ignored font differences.
- 8–9: minor spacing/alignment deviations; a user would not notice anything wrong.
- 6–7: noticeable defects (misalignment, wrong widths, missing radius/shadow/
  gradient, wrong vertical alignment), but fully readable and looks intended.
- 4–5: significant layout defects (broken columns, content overflowing,
  missing images/backgrounds, wrong container width), readable but clearly broken.
- 2–3: badly broken, hard to read or use (e.g. CTA/button unusable, overlaps).
- 0–1: blank, crashed, or unusable.

## Issues
For html_view list concrete problems, in **Brazilian Portuguese**, each
prefixed with a severity tag (`[crítico]`, `[alto]`, `[médio]`, `[baixo]`) and a
class tag (`{layout}` or `{paint}`), e.g.
"[alto]{layout} Botão 'Shop now' 20px mais alto que no Chromium".
Be specific (element, what differs, where, roughly how much). Also give a
`css_suspect` guess (CSS feature responsible) when you can.

## Output
Write the JSON file given in your task: an object mapping each e-mail stem to
`{"score": <0-10, decimals allowed, e.g. 9.5>, "issues": [...], "css_suspect": "..."}`.
Validate it (`python3 -m json.tool`). Final reply: path + average score.
