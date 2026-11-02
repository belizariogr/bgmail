#!/bin/bash
# Runs BGMail on a preview database seeded with the spike's mails, leaving the
# real ~/.config/BGMail/mail.db untouched:
#   - the built-in sample messages (always seeded),
#   - corpus/ (templates) and corpus_real/ (local real mail, if present),
#   - extra *.html dropped in ~/.cache/bgmail-preview/extra/.
# usage: run_preview.sh [--reset]   (--reset rebuilds the preview database)
set -e
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
S=$ROOT/spikes/html_render
P=$HOME/.cache/bgmail-preview
mkdir -p "$P/samples" "$P/extra"
if [ "$1" = "--reset" ] || [ ! -f "$P/mail.db" ]; then
  rm -f "$P/mail.db" "$P"/samples/*.html
  for f in "$S"/corpus/*.html; do cp "$f" "$P/samples/tpl-$(basename "$f")"; done
  for f in "$S"/corpus_real/*.html; do [ -e "$f" ] && cp "$f" "$P/samples/"; done
  for f in "$P"/extra/*.html; do [ -e "$f" ] && cp "$f" "$P/samples/"; done
fi
cd "$ROOT"
cargo build -q -p bgmail
BGMAIL_DB="$P/mail.db" BGMAIL_SAMPLE_DIR="$P/samples" exec target/debug/BGMail
