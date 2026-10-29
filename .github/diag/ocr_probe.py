# TEMPORARY: compare Manga OCR crop strategies on two sample blocks.
import numpy as np
from PIL import Image
from manga_ocr import MangaOcr

mocr = MangaOcr()

def fill_mask(im):
    a = np.asarray(im.convert("RGB")).astype(int)
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    return (b > r + 30) & (b < 200) & (r < 120)

def columns(m, min_share=0.45):
    p = m.sum(0)
    ink = p >= max(1, int(m.shape[0] * 0.01))
    runs, s = [], None
    for i, v in enumerate(list(ink) + [False]):
        if v and s is None:
            s = i
        elif not v and s is not None:
            runs.append((s, i)); s = None
    if not runs:
        return []
    widest = max(b - a for a, b in runs)
    merged = []
    for a, b in runs:
        if merged and a - merged[-1][1] < max(1, int(widest * 0.12)):
            merged[-1] = (merged[-1][0], b)
        else:
            merged.append((a, b))
    widest = max(b - a for a, b in merged)
    return [(a, b) for a, b in merged if b - a >= widest * min_share]

def col_crop(im, m, a, b, pad=0.2):
    rows = np.where(m[:, a:b].any(1))[0]
    w = b - a
    p = max(2, int(np.ceil(w * pad)))
    W, H = im.size
    return im.crop((max(0, a - p), max(0, rows[0] - p), min(W, b + p), min(H, rows[-1] + 1 + p)))

def chunks(img, n):
    W, H = img.size
    return [img.crop((0, H * k // n, W, H * (k + 1) // n)) for k in range(n)]

def note(title, text):
    print(f"::notice title={title}::{text}")

for name in ["b1", "b2"]:
    im = Image.open(f".github/diag/{name}.png").convert("RGB")
    m = fill_mask(im)
    note(f"{name} whole", mocr(im))
    cols = columns(m)
    note(f"{name} ncols", str(cols))
    crops = [col_crop(im, m, a, b) for a, b in reversed(cols)]
    note(f"{name} columns", "|".join(mocr(c) for c in crops))
    note(f"{name} columns-wide-pad", "|".join(mocr(col_crop(im, m, a, b, 0.5)) for a, b in reversed(cols)))
    for n in (2, 3):
        note(f"{name} columns-chunk{n}", "|".join("".join(mocr(p) for p in chunks(c, n)) for c in crops))
    # Whole block padded to a square (no squeeze).
    W, H = im.size
    sq = Image.new("RGB", (max(W, H), max(W, H)), (255, 255, 255))
    sq.paste(im, ((max(W, H) - W) // 2, (max(W, H) - H) // 2))
    note(f"{name} whole-square", mocr(sq))
    note(f"{name} columns-square", "|".join(
        mocr((lambda c: (lambda s: (s.paste(c, ((s.size[0]-c.size[0])//2, 0)), s)[1])(Image.new("RGB", (max(c.size), max(c.size)), (255,255,255))))(c)) for c in crops))
