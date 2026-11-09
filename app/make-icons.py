"""Regenerates every app icon: glossy black oval speech bubble (1.25:1, 80 % of the height), short comma tail
pointing down-left, light grey rim, X Holding X (x-mark.png). Own drawing (user's choice "B", 2026-11-08).
Edge to edge in the square, every size drawn on its own with supersampling. The previous logo (0.1.0 ZIP)
is kept in logo.ico. Run from app/: python make-icons.py
"""
import math

from PIL import Image, ImageChops, ImageDraw, ImageFilter

XMARK = "x-mark.png"  # the X Holding X, white on transparent
ICONS = "src-tauri/icons/"
M = 4096  # geometry canvas; shapes below are in a 1000-unit square
ELL = (500, 400, 500, 400)  # bubble ellipse CX, CY, RX, RY
TAIL = (320, 500, (240, 1000))  # tail leaves the bubble between these x, ends in a tip
X_HEIGHT = .62  # fraction of the bubble height
GLOSS, GLOSS_TO = 150, .5  # glass: white alpha at the top, fading to 0 at this fraction of the height
RIM, RIM_MIN, RIM_GREY = .04, 1.0, (165, 195)  # rim width (fraction of size, min px), grey top -> bottom


def bez(p0, p1, p2, p3, n=80):
    return [tuple((1 - t) ** 3 * a + 3 * (1 - t) ** 2 * t * b + 3 * (1 - t) * t * t * c + t ** 3 * d for a, b, c, d in zip(p0, p1, p2, p3))
            for t in (i / n for i in range(n + 1))]


def masters():
    """bubble, X and gloss masks, cropped to the bubble."""
    CX, CY, RX, RY = ELL
    b0, b1, (tx, ty) = TAIL
    k = M / 1000
    edge = lambda x: CY + RY * math.sqrt(max(0, 1 - ((x - CX) / RX) ** 2)) - 40  # a bit inside, so the tail overlaps
    y0, y1 = edge(b0), edge(b1)
    tail = (bez((b1, y1), (b1, y1 + (ty - y1) * .7), (tx + (b1 - tx) * .6, ty), (tx, ty))
            + bez((tx, ty), (tx + (b0 - tx) * .55, ty - (ty - y0) * .25), (b0 + 10, y0 + (ty - y0) * .45), (b0, y0)))
    bubble = Image.new("L", (M, M))
    d = ImageDraw.Draw(bubble)
    d.ellipse(((CX - RX) * k, (CY - RY) * k, (CX + RX) * k, (CY + RY) * k), fill=255)
    d.polygon([(x * k, y * k) for x, y in tail], fill=255)
    inset = RX * .05
    cap = Image.new("L", (M, M))
    ImageDraw.Draw(cap).ellipse(((CX - RX + inset) * k, (CY - RY + inset) * k, (CX + RX - inset) * k, (CY + RY - inset) * k), fill=255)
    top, h = (CY - RY) * k, 2 * RY * GLOSS_TO * k
    grad = Image.new("L", (M, M))
    gd = ImageDraw.Draw(grad)
    for y in range(int(top), int(top + h)):
        gd.line((0, y, M, y), fill=int(GLOSS * (1 - (y - top) / h)))
    gloss = ImageChops.multiply(grad, cap.filter(ImageFilter.GaussianBlur(inset * k / 3)))
    xsrc = Image.open(XMARK).getchannel("A")
    xh = int(RY * 2 * X_HEIGHT * k)
    xw = int(xsrc.size[0] * xh / xsrc.size[1])
    x = Image.new("L", (M, M))
    x.paste(xsrc.resize((xw, xh), Image.LANCZOS), (int(CX * k - xw / 2), int(CY * k - xh / 2)))
    box = bubble.getbbox()
    return bubble.crop(box), x.crop(box), gloss.crop(box)


def render(bubble, x, gloss, size):
    m = max(2, -(-4096 // size))  # supersampling factor
    w = size * m
    k = w / max(bubble.size)  # edge to edge: the biggest the square allows
    bw, bh = round(bubble.size[0] * k), round(bubble.size[1] * k)
    ox, oy = (w - bw) // 2, (w - bh) // 2

    def place(im):
        c = Image.new("L", (w, w))
        c.paste(im.resize((bw, bh), Image.LANCZOS), (ox, oy))
        return c

    b = place(bubble).point(lambda v: 255 if v > 127 else 0)
    rim = max(RIM * size, RIM_MIN) * m
    inner = b.filter(ImageFilter.GaussianBlur(rim / 2)).point(lambda v: 255 if v > 250 else 0)  # fast erosion
    ring = ImageChops.subtract(b, inner)
    grey = Image.linear_gradient("L").resize((w, w)).point(lambda v: RIM_GREY[0] + (RIM_GREY[1] - RIM_GREY[0]) * v // 255)
    img = Image.new("RGBA", (w, w), (0, 0, 0, 0))
    img.paste((0, 0, 0, 255), mask=b)
    img.paste(Image.merge("RGB", [grey] * 3), mask=ring)
    img.paste((255, 255, 255, 255), mask=ImageChops.multiply(place(gloss), inner))
    img.paste((255, 255, 255, 255), mask=place(x))
    return img.resize((size, size), Image.LANCZOS)


def main():
    parts = masters()
    out = {s: render(*parts, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256, 512)}
    out[512].save("app-icon.png")
    out[512].save(f"{ICONS}icon.png")  # in-app logo (toolbar, welcome)
    out[512].save(f"{ICONS}icon.icns")
    for name, s in (("32x32", 32), ("64x64", 64), ("128x128", 128), ("128x128@2x", 256)):
        out[s].save(f"{ICONS}{name}.png")
    out[32].save(f"{ICONS}tray.png")  # 32 px = the tray slot at 200 %
    sizes = [16, 20, 24, 32, 40, 48, 64, 256]  # every size Windows asks for at 100-250 % scaling
    out[256].save(f"{ICONS}icon.ico", sizes=[(s, s) for s in sizes], append_images=[out[s] for s in sizes if s != 256])


if __name__ == "__main__":
    main()
