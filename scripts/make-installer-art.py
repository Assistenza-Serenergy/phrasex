"""Installer art: a black "software box" with white edges and the Phrase X X on the front.
Writes app/src-tauri/installer/ (installer.ico, header.bmp 150x57, sidebar.bmp 164x314) and
scripts/installer-preview.png. Run from anywhere: python scripts/make-installer-art.py
"""
import os

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "app", "src-tauri", "installer")
XMARK = Image.open(os.path.join(ROOT, "app", "x-mark.png")).convert("RGBA")  # white X on transparent
SS = 4  # supersampling

# box in a 1000-unit square: front face + depth going up-right (oblique view)
FRONT = (120, 300, 700, 880)
DX, DY = 180, -180
FRONT_FILL, TOP_FILL, SIDE_FILL = (14, 14, 14), (44, 44, 44), (4, 4, 4)


def box(size, edge_units):
    """The box as RGBA, `size` px square; `edge_units` = white edge width in 1000-units (thicker when small)."""
    m = size * SS
    k = m / 1000
    P = lambda pts: [(x * k, y * k) for x, y in pts]
    x0, y0, x1, y1 = FRONT
    front = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    top = [(x0, y0), (x0 + DX, y0 + DY), (x1 + DX, y0 + DY), (x1, y0)]
    side = [(x1, y0), (x1 + DX, y0 + DY), (x1 + DX, y1 + DY), (x1, y1)]
    im = Image.new("RGBA", (m, m))
    d = ImageDraw.Draw(im)
    for face, fill in ((top, TOP_FILL), (side, SIDE_FILL), (front, FRONT_FILL)):
        d.polygon(P(face), fill=fill)
    # the X, centred on the front face
    w = int((x1 - x0) * .62 * k)
    xm = XMARK.resize((w, int(w * XMARK.height / XMARK.width)), Image.LANCZOS)
    im.alpha_composite(xm, (int((x0 + x1) / 2 * k - xm.width / 2), int((y0 + y1) / 2 * k - xm.height / 2)))
    # white edges: outline + the three inner edges
    e = max(1, round(edge_units * k))
    outline = [(x0, y1), (x0, y0), (x0 + DX, y0 + DY), (x1 + DX, y0 + DY), (x1 + DX, y1 + DY), (x1, y1), (x0, y1)]
    d.line(P(outline), fill="white", width=e, joint="curve")
    d.line(P([(x0, y0), (x1, y0), (x1, y1)]), fill="white", width=e, joint="curve")
    d.line(P([(x1, y0), (x1 + DX, y0 + DY)]), fill="white", width=e)
    for x, y in P(outline):
        d.ellipse((x - e / 2, y - e / 2, x + e / 2, y + e / 2), fill="white")  # round corners
    return im.resize((size, size), Image.LANCZOS)


def icon_sizes():
    # thicker edges at small sizes so they stay visible
    return {s: box(s, 22 if s >= 64 else 34 if s >= 32 else 48) for s in (16, 24, 32, 48, 64, 128, 256)}


def banner(w, h, box_px, text_px, bg):
    im = Image.new("RGB", (w, h), bg)
    font = ImageFont.truetype(r"C:\Windows\Fonts\calibrib.ttf", text_px)
    return im, ImageDraw.Draw(im), font, box(box_px, 22)


def main():
    os.makedirs(OUT, exist_ok=True)
    icons = icon_sizes()
    icons[256].save(os.path.join(OUT, "installer.ico"), sizes=[(s, s) for s in icons], append_images=list(icons.values()))

    # header (inner pages, top right): white strip, box at the right
    header, d, font, b = banner(150, 57, 52, 20, (255, 255, 255))
    header.paste(b, (150 - 54, 3), b)
    d.text((8, 16), "Phrase X", fill=(0, 0, 0), font=font)
    header.save(os.path.join(OUT, "header.bmp"))

    # sidebar (welcome / finish pages): black, big box, name under it
    sidebar, d, font, b = banner(164, 314, 136, 26, (0, 0, 0))
    sidebar.paste(b, (14, 70), b)
    tw = d.textlength("Phrase X", font=font)
    d.text(((164 - tw) / 2, 222), "Phrase X", fill=(255, 255, 255), font=font)
    sidebar.save(os.path.join(OUT, "sidebar.bmp"))

    # preview sheet
    label = ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf", 16)
    sheet = Image.new("RGB", (1120, 400), (32, 32, 32))
    d = ImageDraw.Draw(sheet)
    x = 20
    for s, bg in ((256, (60, 60, 60)), (48, (70, 30, 28)), (32, (70, 30, 28)), (16, (70, 30, 28))):
        tile = Image.new("RGB", (max(s, 60) + 20, max(s, 60) + 20), bg)
        tile.paste(icons[s], ((tile.width - s) // 2, (tile.height - s) // 2), icons[s])
        sheet.paste(tile, (x, 40))
        d.text((x, 14), f"icona {s} px", fill="white", font=label)
        x += tile.width + 20
    sheet.paste(header.resize((300, 114), Image.NEAREST), (x, 40))
    d.text((x, 14), "intestazione (x2)", fill="white", font=label)
    sheet.paste(sidebar, (x + 320, 40))
    d.text((x + 320, 14), "barra laterale", fill="white", font=label)
    sheet.save(os.path.join(ROOT, "scripts", "installer-preview.png"))


if __name__ == "__main__":
    main()
