#!/bin/bash
# usage: ldiff.sh <stem> [threshold] [max] [width]  — layout diff Chromium vs Blitz for a corpus mail
R=/home/belizario/.t3/worktrees/bgmail/t3-fd5eeea7; S=$R/spikes/html_render
mkdir -p /tmp/ldiff
(cd $R && cargo build -q -p html_view --example layout_dump)
(cd $S/corpus_build && node layout_dump.mjs $S/corpus/$1.html ${4:-800}) > /tmp/ldiff/$1.chrome.tsv
$R/target/debug/examples/layout_dump $S/corpus/$1.html ${4:-800} > /tmp/ldiff/$1.blitz.tsv 2>/dev/null
python3 -I $S/tools/layout_diff.py /tmp/ldiff/$1.chrome.tsv /tmp/ldiff/$1.blitz.tsv ${2:-2} ${3:-25}
