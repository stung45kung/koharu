# TEMPORARY: Manga OCR accuracy on three reference blocks.
import numpy as np
from PIL import Image
from manga_ocr import MangaOcr

TRUTH = {
    "t1": "へへ…似合ってるでぇ嬢ちゃん",
    "t2": "ウチは昼、持ち帰りのカウンターだけの営業やさかい",
    "t3": "作ったもん渡すだけで簡単やろ？無口な嬢ちゃんにもってこいや",
}
mocr = MangaOcr()

def norm(s):
    return s.replace("．．．", "…").replace("...", "…")

def dist(a, b):
    d = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        p, d[0] = d[0], i
        for j, cb in enumerate(b, 1):
            p, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, p + (ca != cb))
    return d[-1]

def fill_mask(a):
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    return (b > r + 30) & (b < 210) & (r < 120)

def columns(m):
    p = m.sum(0)
    nz = p[p > 0]
    thr = max(1, m.shape[0] * 0.01, 0.15 * np.median(nz))
    ink = list(p >= thr) + [False]
    runs, s = [], None
    for i, v in enumerate(ink):
        if v and s is None: s = i
        elif not v and s is not None: runs.append([s, i]); s = None
    widest = max(b - a for a, b in runs)
    merged = []
    for a, b in runs:
        if merged and a - merged[-1][1] < max(1, int(widest * 0.05)): merged[-1][1] = b
        else: merged.append([a, b])
    widest = max(b - a for a, b in merged)
    return [c for c in merged if c[1] - c[0] >= widest * 0.45]

def crops(im, m, cols):
    W, H = im.size
    out = []
    for i, (a, b) in enumerate(cols):
        pad = max(2, int(np.ceil((b - a) * 0.2)))
        lo = cols[i - 1][1] + (a - cols[i - 1][1]) // 2 if i else 0
        hi = b + (cols[i + 1][0] - b + 1) // 2 if i + 1 < len(cols) else W
        rows = np.where(m[:, a:b].any(1))[0]
        out.append(im.crop((max(a - pad, lo), max(0, rows[0] - pad), min(b + pad, hi), min(H, rows[-1] + 1 + pad))))
    return out[::-1]

total = {"whole": 0, "columns": 0}
n = 0
for name, truth in TRUTH.items():
    im = Image.open(f".github/diag/{name}.png").convert("RGB")
    a = np.asarray(im).astype(int)
    m = fill_mask(a)
    cols = columns(m)
    whole = norm(mocr(im))
    col = norm("".join(mocr(c) for c in crops(im, m, cols)))
    n += len(truth)
    for k, v in (("whole", whole), ("columns", col)):
        e = dist(v, truth); total[k] += e
        print(f"::notice title={name} {k} ({len(cols)} cols) errors={e}/{len(truth)}::{v}")
for k, e in total.items():
    print(f"::notice title=TOTAL {k}::char accuracy {100 * (1 - e / n):.1f}% ({e} edits / {n} chars)")
