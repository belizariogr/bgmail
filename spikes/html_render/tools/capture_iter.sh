#!/bin/bash
# Captures every corpus mail with html_view and builds Chromium | html_view | previous sheets.
# usage: capture_iter.sh <iteration-number> [previous-iteration-number]
set -e
ROOT=/home/belizario/.t3/worktrees/bgmail/t3-fd5eeea7
S=$ROOT/spikes/html_render
IT=$1; PREV=${2:-}
cd $ROOT
cargo build -q -p html_view --example viewer
pkill -x viewer || true
rm -rf $S/out/html_view_it$IT
target/debug/examples/viewer --capture $S/out/html_view_it$IT $S/corpus/*.html > $S/out/html_view_it$IT.tsv 2>/tmp/cap_$IT.log
rm -rf $S/out_hv$IT && mkdir $S/out_hv$IT
ln -s ../out/chromium $S/out_hv$IT/chromium
ln -s ../out/html_view_it$IT $S/out_hv$IT/blitz
if [ -n "$PREV" ]; then ln -s ../out/html_view_it$PREV $S/out_hv$IT/litehtml; else ln -s ../out/blitz_fixed $S/out_hv$IT/litehtml; fi
$S/engines/target/release/compare $S/out_hv$IT > $S/out_hv$IT/scores.tsv
awk -F'\t' 'NR>1{a+=$4;b+=$6;n++}END{printf "mean ssim it %.3f  prev %.3f (n=%d)\n",a/n,b/n,n}' $S/out_hv$IT/scores.tsv
