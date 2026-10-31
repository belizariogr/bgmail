"""Aligns Chromium and Blitz element boxes (document order) and prints where
they diverge. usage: layout_diff.py chrome.tsv blitz.tsv [threshold_px] [max]"""
import difflib, sys
SKIP = {'tbody', 'thead', 'tfoot', 'tr'}  # boxless in Blitz
def load(p):
    return [r for r in (l.rstrip('\n').split('\t') for l in open(p) if l.strip()) if r[0] not in SKIP]
c, b = load(sys.argv[1]), load(sys.argv[2])
th = float(sys.argv[3]) if len(sys.argv) > 3 else 2
mx = int(sys.argv[4]) if len(sys.argv) > 4 else 25
print(f"elements chrome={len(c)} blitz={len(b)}")
shown = 0
prev_dy = 0
INLINE = {'a','span','b','strong','em','i','u','font','sup','sub','small','br','code','s','strike','label','abbr','mark','wbr','o:p'}
# Align by tag sequence: the reader replaces unavailable images with spans
# (icon + alt text), so the two lists are not index-aligned.
blocks = difflib.SequenceMatcher(
    None, [r[0] for r in c], [r[0] for r in b], autojunk=False).get_matching_blocks()
for i, j in ((ci + k, bi + k) for ci, bi, n in blocks for k in range(n)):
    ce, be = c[i], b[j]
    if ce[0] in INLINE and be[3] == '0' and be[4] == '0':
        continue  # inline boxes are not laid out as boxes in Blitz
    cx, cy, cw, ch = map(float, ce[1:]); bx, by, bw, bh = map(float, be[1:])
    dy = by - cy
    # report size mismatches and *changes* in vertical drift (where error is introduced)
    if abs(bh - ch) > th or abs(bw - cw) > th or abs(bx - cx) > th or abs(dy - prev_dy) > th:
        print(f"#{i:<4} {ce[0]:<6} chrome x={cx:.0f} y={cy:.0f} w={cw:.0f} h={ch:.0f} | blitz x={bx:.0f} y={by:.0f} w={bw:.0f} h={bh:.0f} | dy={dy:+.0f} dh={bh-ch:+.0f} dw={bw-cw:+.0f}")
        shown += 1
        if shown >= mx: break
    prev_dy = dy
