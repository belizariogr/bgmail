"""Builds REPORT.md (per-email issues + scores) and out/report.html (with the
side-by-side review sheets) from out/reviews/all.json and the metrics TSVs."""
import html, json, os, re, statistics as st

rows = json.load(open("out/reviews/all.json"))
scores = {l.split("\t")[0]: l.rstrip("\n").split("\t") for l in open("out/scores.tsv") if not l.startswith("email")}
times = {e: {l.split("\t")[0]: l.split("\t") for l in open(f"out/{e}_times.tsv")} for e in ("blitz", "litehtml")}
sheets = sorted(os.listdir("out/sheets"))

def source(n):
    if n.startswith("re-0"): return "react-email (temas Tailwind)"
    if n.startswith("re-"): return "react-email (clones de marcas)"
    return {"mjml": "MJML", "postmark": "Postmark", "mailgun": "Mailgun", "cerberus": "Cerberus",
            "leemunroe": "Lee Munroe", "client": "Gerado por cliente de e-mail"}[n.split("-")[0]]

def avg(e, sel=None):
    v = [r[e]["score"] for r in rows if sel is None or sel(r)]
    return st.mean(v)

md = []
md.append("# Fidelidade de renderização de e-mail: Blitz × litehtml × Chromium\n")
md.append("Gerado por `tools/build_report.py`. Metodologia, ressalvas e como reproduzir em `README.md`.\n")
md.append("## Resumo\n")
md.append("| | Blitz | litehtml |\n|---|---|---|")
md.append(f"| Nota média (0–10) | **{avg('blitz'):.1f}** | **{avg('litehtml'):.1f}** |")
md.append(f"| Mediana | {st.median([r['blitz']['score'] for r in rows])} | {st.median([r['litehtml']['score'] for r in rows])} |")
for lo, label in ((8, "≥ 8 (usuário não nota problema)"), (6, "≥ 6 (legível, parece intencional)")):
    md.append(f"| E-mails com nota {label} | {sum(r['blitz']['score']>=lo for r in rows)}/47 | {sum(r['litehtml']['score']>=lo for r in rows)}/47 |")
md.append(f"| E-mails com nota ≤ 4 (quebrado) | {sum(r['blitz']['score']<=4 for r in rows)}/47 | {sum(r['litehtml']['score']<=4 for r in rows)}/47 |")
w = sum(r['blitz']['score'] > r['litehtml']['score'] for r in rows); l = sum(r['litehtml']['score'] > r['blitz']['score'] for r in rows)
md.append(f"| Vitórias diretas | {w} | {l} (empates: {47-w-l}) |\n")
md.append("### Por origem\n")
md.append("| Origem | n | Blitz | litehtml |\n|---|---|---|---|")
for s in sorted({source(r['email']) for r in rows}):
    sel = lambda r, s=s: source(r['email']) == s
    md.append(f"| {s} | {sum(map(sel, rows))} | {avg('blitz', sel):.1f} | {avg('litehtml', sel):.1f} |")
md.append("")
md.append("## Por e-mail\n")
md.append("Colunas: nota (0–10) e SSIM (similaridade estrutural aproximada com o Chromium, 1 = idêntico; serve só de pista: a diferença de fontes pesa bastante).\n")
for r in rows:
    n = r["email"]; sc = scores[n]
    md.append(f"### `{n}` · {source(n)}\n")
    md.append(f"{r.get('summary_pt','')}\n")
    md.append(f"| | Nota | SSIM | Altura (Chromium {sc[1]}px) |\n|---|---|---|---|")
    md.append(f"| Blitz | **{r['blitz']['score']}** | {sc[3]} | {sc[2]}px |")
    md.append(f"| litehtml | **{r['litehtml']['score']}** | {sc[5]} | {sc[4]}px |\n")
    for e, label in (("blitz", "Blitz"), ("litehtml", "litehtml")):
        issues = r[e]["issues"]
        md.append(f"**{label}:** " + ("nenhum problema relevante." if not issues else ""))
        for i in issues:
            md.append(f"- {i}")
        if r[e].get("css_suspect"):
            md.append(f"- _CSS suspeito: {r[e]['css_suspect']}_")
        md.append("")
open("REPORT.md", "w").write("\n".join(md))

# HTML report with sheets.
h = ["<!doctype html><meta charset=utf-8><title>Blitz × litehtml × Chromium</title><style>",
     "body{font:14px system-ui,sans-serif;margin:24px;max-width:1700px;color:#222}h2{margin-top:48px;border-top:1px solid #ddd;padding-top:16px}",
     ".s{font-weight:700;padding:2px 8px;border-radius:4px;color:#fff}.b{background:#e67814}.l{background:#1e6edc}",
     "img{max-width:100%;display:block;margin:8px 0;border:1px solid #ccc}ul{margin:4px 0 8px}.cols{display:grid;grid-template-columns:1fr 1fr;gap:24px}",
     "</style><h1>Fidelidade de renderização de e-mail</h1>",
     f"<p>Colunas das imagens: <b>Chromium</b> (referência) | <span class='s b'>Blitz</span> | <span class='s l'>litehtml</span>. ",
     f"Média: Blitz <b>{avg('blitz'):.1f}</b>, litehtml <b>{avg('litehtml'):.1f}</b>. Detalhes em REPORT.md.</p>"]
for r in rows:
    n = r["email"]
    h.append(f"<h2 id='{n}'>{html.escape(n)} <small>· {source(n)}</small></h2><p>{html.escape(r.get('summary_pt',''))}</p><div class=cols>")
    for e, c, label in (("blitz", "b", "Blitz"), ("litehtml", "l", "litehtml")):
        h.append(f"<div><span class='s {c}'>{label}: {r[e]['score']}/10</span><ul>")
        h += [f"<li>{html.escape(i)}</li>" for i in r[e]["issues"]] or ["<li>nenhum problema relevante</li>"]
        h.append("</ul></div>")
    h.append("</div>")
    h += [f"<img loading=lazy src='sheets/{s}'>" for s in sheets if s.startswith(n + "__")]
open("out/report.html", "w").write("\n".join(h))
print("ok")
