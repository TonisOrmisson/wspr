# Regenerate: python3 res/make_icon.py res/wspr.ico (needs Pillow).
# Draws the wspr app icon (white microphone on a blue rounded square) into a multi-size .ico.
import sys
from PIL import Image, ImageDraw

def draw(n):
    s = 4 * n  # supersample, then downscale for smooth edges
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=(37, 99, 235, 255))
    w = s * 0.075
    white = (255, 255, 255, 255)
    d.rounded_rectangle([s * 0.38, s * 0.16, s * 0.62, s * 0.58], radius=s * 0.12, fill=white)  # capsule
    d.arc([s * 0.27, s * 0.30, s * 0.73, s * 0.70], start=0, end=180, fill=white, width=int(w))  # holder
    d.line([s * 0.5, s * 0.70, s * 0.5, s * 0.82], fill=white, width=int(w))  # stem
    d.line([s * 0.37, s * 0.82, s * 0.63, s * 0.82], fill=white, width=int(w))  # base
    return im.resize((n, n), Image.LANCZOS)

sizes = [16, 24, 32, 48, 64, 128, 256]
big = draw(256)
big.save(sys.argv[1], sizes=[(n, n) for n in sizes], append_images=[draw(n) for n in sizes[:-1]])
big.save(sys.argv[1].replace(".ico", ".png"))
# Raw RGBA for the tray and window icons, so the app needs no image decoder.
import os
out = os.path.dirname(sys.argv[1])
for n in (32, 64):
    open(os.path.join(out, f"icon-{n}.rgba"), "wb").write(draw(n).tobytes())
