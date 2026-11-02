# Review brief: HTML e-mail render fidelity (Blitz and litehtml vs Chromium)

Context: we are choosing an HTML/CSS engine to render e-mails natively inside a
Rust desktop mail client. Each e-mail was rendered at 800px viewport width by:
- **Chromium** (reference, left column, dark-grey header strip)
- **Blitz** (middle column, ORANGE header strip)
- **litehtml** (right column, BLUE header strip)

Review sheets: `out/sheets/<email>__<N>.png` (N = 0,1,2… vertical segments of
the same e-mail, 1100px tall each, columns scaled to 520px). View EVERY segment
of every e-mail in your batch. If a detail is unclear, you may also open the
full-resolution renders at `out/{chromium,blitz,litehtml}/<email>.png`.
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

## Score each engine 0–10 against Chromium
- 10: indistinguishable apart from ignored font differences.
- 8–9: minor spacing/alignment deviations; a user would not notice anything wrong.
- 6–7: noticeable defects (misalignment, wrong widths, missing radius/shadow/
  gradient, wrong vertical alignment), but fully readable and looks intended.
- 4–5: significant layout defects (broken columns, content overflowing,
  missing images/backgrounds, wrong container width), readable but clearly broken.
- 2–3: badly broken, hard to read or use (e.g. CTA/button unusable, overlaps).
- 0–1: blank, crashed, or unusable.

## Issues
For each engine list concrete problems, in **Brazilian Portuguese**, each
prefixed with a severity tag: `[crítico]`, `[alto]`, `[médio]`, `[baixo]`.
Be specific (which element, what is wrong, where), e.g.
"[alto] Container de 600px ignorado: conteúdo ocupa os 800px inteiros".
Do not list ignored items. Empty list if nothing is wrong.
Also add a one-line `"css_suspect"` guess of the CSS feature(s) responsible
when obvious (e.g. "max-width em tabela", "vertical-align em td",
"border-radius", "background-image", "display:inline-block").

## Output
Write a JSON file (path given in your task) with exactly this shape, one entry
per e-mail in your batch:

```json
[
  {
    "email": "re-apple-receipt",
    "summary_pt": "Recibo com tabelas aninhadas, colunas de dados e rodapé longo.",
    "blitz":    { "score": 7, "issues": ["[médio] ..."], "css_suspect": "..." },
    "litehtml": { "score": 5, "issues": ["[alto] ..."],  "css_suspect": "..." }
  }
]
```
Validate the JSON (e.g. `python3 -m json.tool <file>`) before finishing.
Your final reply: just the file path and a one-line average per engine.
