import os
from PIL import Image
os.makedirs("crops", exist_ok=True)
def shrink(im):
    short = min(im.size)
    if short <= 160: return im
    s = 160 / short
    return im.resize((max(1, round(im.width * s)), max(1, round(im.height * s))), Image.BILINEAR)
for n in ("t1", "t2", "t3"):
    im = Image.open(f".github/diag/{n}.png").convert("RGB")
    for s in (3, 4):
        big = im.resize((im.width * s, im.height * s), Image.LANCZOS)
        shrink(big).save(f"crops/{n}_x{s}_shrunk.png")
