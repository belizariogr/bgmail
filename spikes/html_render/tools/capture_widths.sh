#!/bin/bash
# Captures every corpus mail with html_view at several column widths and scores
# each width against the Chromium reference rendered at the same width
# (corpus_build/render_chromium.mjs <corpus> out/chromium_w<W> <W>).
# usage: capture_widths.sh <iteration> [widths...]   (default: 375 600 800 1280)
# CORPUS=corpus_real TAG=real_ captures another corpus (references in
# out/${TAG}chromium_w<W>, results in out/${TAG}html_view_it<n>_w<W> and
# out_${TAG}hv<n>_w<W>). Re-apply the Hyprland capture rule first (see README).
set -e
ROOT=/home/belizario/.t3/worktrees/bgmail/t3-fd5eeea7
S=$ROOT/spikes/html_render
IT=$1; shift
CORPUS=${CORPUS:-corpus}
TAG=${TAG:-}
WIDTHS=${*:-375 600 800 1280}
cd $ROOT
cargo build -q -p html_view --example viewer
for W in $WIDTHS; do
  pkill -x viewer || true
  OUT=$S/out/${TAG}html_view_it${IT}_w$W
  rm -rf $OUT
  target/debug/examples/viewer --width $W --capture $OUT $S/$CORPUS/*.html > $OUT.tsv 2>/tmp/cap_${IT}_$W.log
  CMP=$S/out_${TAG}hv${IT}_w$W
  rm -rf $CMP && mkdir $CMP
  # Mail wider than the column overflows; Chromium's full-page shot then grows
  # wider than the viewport. Compare what a reader sees: the first W columns.
  REF=${TAG}chromium_w$W
  mkdir -p $S/out/${REF}_vp
  for f in $S/out/$REF/*.png; do
    [ $S/out/${REF}_vp/$(basename $f) -nt $f ] || magick $f -crop ${W}x+0+0 +repage $S/out/${REF}_vp/$(basename $f)
  done
  ln -s ../out/${REF}_vp $CMP/chromium
  ln -s ../out/${TAG}html_view_it${IT}_w$W $CMP/blitz
  # The compare tool wants a third column; reuse html_view's own capture.
  ln -s ../out/${TAG}html_view_it${IT}_w$W $CMP/litehtml
  $S/engines/target/release/compare $CMP > $CMP/scores.tsv
  awk -F'\t' -v w=$W 'NR>1{a+=$4;n++; if($4<m||m=="")m=$4}END{printf "w=%s mean ssim %.3f  min %.3f (n=%d)\n",w,a/n,m,n}' $CMP/scores.tsv
done
