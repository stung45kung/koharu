import os
from PIL import Image
os.makedirs("crops", exist_ok=True)
for n in ("t1", "t2", "t3"):
    im = Image.open(f".github/diag/{n}.png").convert("RGB")
    for s in (1, 2, 3, 4):
        im.resize((im.width * s, im.height * s), Image.LANCZOS).save(f"crops/{n}_x{s}.png")
