# Spike: HTML e-mail engine fidelity (Blitz × litehtml × Chromium)

Throwaway evaluation, run 2026-10-31, to choose a native engine that can replace
the CEF off-screen webview in the reader. Results: [`REPORT.md`](REPORT.md)
(per-email issues and scores) and `out/report.html` (same data with the
side-by-side render sheets; generated, not committed).

## Corpus (47 e-mails)

| Source | n | What it represents |
|---|---|---|
| react-email demo, brand clones (Apple, Nike, Stripe, Airbnb, Amazon, Vercel, …) | 22 | Modern transactional e-mail from SaaS/retail |
| react-email demo, Tailwind themes | 6 | Modern marketing (gradients, radius, image grids) |
| MJML official templates | 8 | Marketing newsletters (MJML = what many ESPs emit) |
| Postmark templates (inlined) | 4 | Transactional (receipt, invoice, welcome, comment) |
| Mailgun, Lee Munroe, Cerberus (hybrid + responsive) | 4 | Classic responsive patterns (`display:block` td, ghost tables) |
| Client-generated (Gmail reply, Outlook desktop/Word, Apple Mail) | 3 | Person-to-person mail; hand-written to match each client's real markup |

Limitations: templates, not captured inbox mail (no real newsletters with
trackers); no RTL/CJK content; no dark-mode check.

## Method

- `corpus_build/`: renders react-email (`render_react.tsx`) and MJML to HTML;
  `render_chromium.mjs` is the reference renderer (headless Chromium, offline).
- `tools/prepare_corpus.py`: downloads every remote image once and rewrites it to
  `corpus/assets/` (identical offline input for all engines); removes remote web
  fonts (BGMail blocks remote content by default; every engine falls back to
  system fonts); fills Postmark placeholders.
- `engines/`: standalone Cargo workspace (never part of the BGMail build):
  - `blitz_render`: Blitz `e42ee1f` (2026-10-30), `floats` feature on, CPU
    raster via `anyrender_vello_cpu`. **Harness workaround:** forces the HTML
    parser (Blitz sniffs XHTML doctypes / `xmlns` roots and parses as XML,
    which drops the HTML UA stylesheet; mail is always parsed as HTML by browsers).
  - `litehtml_render`: litehtml-rs `662ed3a` (vendors litehtml `8836bc1`,
    2026-01-10) with the crate's `prepare_html` pipeline and default master CSS.
    **Harness patch:** `patches/litehtml-rs-pixbuf-image-scaling.patch` makes the
    crate's sample CPU painter scale/tile images to litehtml's computed box (it
    drew them at natural size: a painter bug, not a layout one).
  - `compare`: coarse SSIM vs Chromium + review sheets (Chromium | Blitz | litehtml).
- Viewport 800px, scale 1×. Scores (0–10) and issues come from visual review of
  every sheet against the rubric in `out/reviews/RUBRIC.md`; SSIM is only a hint.
- `repros/`: minimal reproductions of the top bug of each engine.
- Engine fixes evaluated afterwards (REPORT.md addenda), applied to local clones
  via `[patch]` in `engines/Cargo.toml`:
  `patches/litehtml-table-maxwidth-align-intrinsic.patch` (litehtml C++) and
  `patches/blitz-anonymous-cell-table-align.patch` (Blitz; its 471 tests pass).

## Reproduce

```sh
cd corpus_build && npm i && npx tsx --tsconfig tsconfig.json render_react.tsx '<json picks>' ../corpus_raw
npx mjml <template>.mjml -o ../corpus_raw/mjml-<name>.html          # see git history of the run
python3 -I ../tools/prepare_corpus.py ../corpus_raw ../corpus <react-email>/apps/demo/emails/static
node render_chromium.mjs ../corpus ../out/chromium      # [width] 3rd arg, default 800
cd ../engines && cargo build --release   # needs the patched litehtml-rs clone (see Cargo.toml [patch])
./target/release/blitz_render ../corpus ../out/blitz > ../out/blitz_times.tsv
./target/release/litehtml_render ../corpus ../out/litehtml > ../out/litehtml_times.tsv
./target/release/compare ../out > ../out/scores.tsv
python3 -I ../tools/build_report.py      # after reviews in out/reviews/batch_*.json → all.json
```

## html_view loop (native GPUI platform)

- `tools/capture_iter.sh <n> [prev]`: builds `crates/html_view`'s `viewer`,
  captures every corpus mail from the real GPUI window into
  `out/html_view_it<n>/` and builds `out_hv<n>/sheets` (Chromium | html_view |
  previous iteration) with SSIM in `out_hv<n>/scores.tsv`. Needs Wayland/
  Hyprland (`grim`, `hyprctl`); disable window effects for the viewer with a
  runtime rule (removed by `hyprctl reload`):
  `hyprctl eval 'hl.window_rule({ match = { class = "^html_view_viewer$" }, tag = "-default-opacity", opacity = "1 1", no_blur = true, rounding = 0, no_dim = true, border_size = 0 })'`.
- `tools/capture_widths.sh <n> [widths…]` (default 375 600 800 1280): same
  loop per viewport width against `out/chromium_w<W>/` (render those with
  `node render_chromium.mjs ../corpus ../out/chromium_w<W> <W>`, one width at
  a time). Chromium's full-page shots of overflowing mail are wider than the
  viewport; they are cropped to `out/chromium_w<W>_vp/` (what a reader sees).
  Results in `out_hv<n>_w<W>/`. The Hyprland rule also needs
  `float = true, size = "1400 1000"` so 1280px fits.
- `render_chromium.mjs` and `layout_dump.mjs` load doctype-less mail through a
  sibling copy with `<!DOCTYPE html>` (html_view renders it in standards mode).
- `tools/ldiff.sh <stem> [px] [max] [width]`: element-box diff Chromium vs
  Blitz (`corpus_build/layout_dump.mjs` + `layout_dump` example, rebuilt);
  finds where layout drifts. `corpus_build/attrs.mjs <file> <idx>...` prints
  computed styles.
- Reviews per round: `out/reviews_hv/` (800px, `RUBRIC_HV.md`, `final.json`)
  and `out/reviews_widths/` (all widths, `RUBRIC_WIDTHS.md`, `final.json`).
