"""Normalizes the raw corpus so every engine receives identical, offline input.

- Remote images (src=, background=, CSS url()) are downloaded once and rewritten
  to relative `assets/` paths, so network variance never affects a comparison.
- Remote web fonts are removed: BGMail blocks remote content by default and most
  mail clients ignore web fonts, so all engines fall back to the same font stack.
- Postmark's Mustachio placeholders are filled with sample values.
"""
import hashlib, os, re, shutil, sys, urllib.request

RAW, OUT, STATIC = sys.argv[1], sys.argv[2], sys.argv[3]
ASSETS = os.path.join(OUT, "assets")
os.makedirs(ASSETS, exist_ok=True)
cache = {}

def fetch(url):
    if url in cache:
        return cache[url]
    ext = os.path.splitext(url.split("?")[0])[1][:5] or ".img"
    name = hashlib.sha1(url.encode()).hexdigest()[:12] + ext
    path = os.path.join(ASSETS, name)
    if not os.path.exists(path):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
            with urllib.request.urlopen(req, timeout=20) as r, open(path, "wb") as f:
                f.write(r.read())
        except Exception as e:
            print("  ! fetch failed", url, e)
            cache[url] = None
            return None
    cache[url] = "assets/" + name
    return cache[url]

def local_static(path):
    rel = path[len("/static/"):]
    src = os.path.join(STATIC, rel)
    if not os.path.exists(src):
        print("  ! missing static", path)
        return None
    name = rel.replace("/", "_")
    shutil.copy(src, os.path.join(ASSETS, name))
    return "assets/" + name

def rewrite_url(url):
    url = url.strip().strip("'\"")
    if url.startswith("//"):
        url = "https:" + url
    if url.startswith(("http://", "https://")):
        return fetch(url)
    if url.startswith("/static/"):
        return local_static(url)
    return url

def fill_mustache(html):
    html = re.sub(r"\{\{#each[^}]*\}\}(.*?)\{\{/each\}\}", lambda m: m.group(1) * 2, html, flags=re.S)
    html = re.sub(r"\{\{[#^/][^}]*\}\}", "", html)
    samples = {"name": "Mariana", "product_name": "Acme Cloud", "amount": "$49.00", "total": "$98.00",
               "date": "Oct 30, 2026", "description": "Pro plan (monthly)", "action_url": "https://example.com/action",
               "receipt_id": "R-20482", "invoice_id": "INV-0042", "due_date": "Nov 15, 2026"}
    def var(m):
        key = m.group(1).strip().split(".")[-1]
        return samples.get(key, key.replace("_", " ").title())
    return re.sub(r"\{\{\{?\s*([^}]+?)\s*\}?\}\}", var, html)

for fname in sorted(os.listdir(RAW)):
    if not fname.endswith(".html"):
        continue
    print(fname)
    html = open(os.path.join(RAW, fname), encoding="utf-8", errors="replace").read()
    if fname.startswith("postmark-"):
        html = fill_mustache(html)
    # Remote fonts.
    html = re.sub(r"<link[^>]+fonts\.(googleapis|gstatic)[^>]*>", "", html, flags=re.I)
    html = re.sub(r"@import\s+url\([^)]*\)\s*;?", "", html, flags=re.I)
    html = re.sub(r"@font-face\s*\{[^}]*\}", "", html, flags=re.I)
    # Images in attributes and CSS.
    def attr(m):
        new = rewrite_url(m.group(3))
        return f'{m.group(1)}={m.group(2)}{new or ""}{m.group(2)}'
    html = re.sub(r'\b(src|background)=(["\'])(.*?)\2', attr, html, flags=re.I)
    def css(m):
        new = rewrite_url(m.group(1))
        return f"url('{new or ''}')"
    html = re.sub(r"url\(([^)]*)\)", css, html)
    open(os.path.join(OUT, fname), "w", encoding="utf-8").write(html)
