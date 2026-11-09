"""The icon files in every placement (as Windows / the app show them at 200 %), next to PhraseExpress for size."""
import os

from PIL import Image, ImageDraw, ImageFont

S = os.path.dirname(os.path.abspath(__file__))  # preview saved next to this script
I = r'C:\Users\Marco\Desktop\PhraseX\app\src-tauri\icons' + '\\'
PE = r'C:\Program Files (x86)\PhraseExpress\phraseexpress.exe'

font = ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf", 22)
small = ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf", 16)
ico = Image.open(I + 'icon.ico')
logo = Image.open(I + 'icon.png').convert('RGBA')
# label, image, background
PLACES = [("barra titolo", ico.ico.getimage((32, 32)), (32, 32, 32)), ("app, in alto", logo.resize((46, 46), Image.LANCZOS), (11, 11, 11)),
          ("app, benvenuto", logo.resize((136, 136), Image.LANCZOS), (0, 0, 0)), ("barra Windows", ico.ico.getimage((48, 48)), (70, 30, 28)),
          ("vicino al Wi-Fi", Image.open(I + 'tray.png'), (70, 30, 28))]
if os.path.exists(PE):  # PhraseExpress's own 32 px icon, only to compare the size, never shipped
    tmp = os.path.join(S, 'pe32.png')
    os.system(f'powershell -NoProfile -Command "Add-Type -AssemblyName System.Drawing; '
              f'[System.Drawing.Icon]::ExtractAssociatedIcon(\'{PE}\').ToBitmap().Save(\'{tmp}\')"')
    PLACES.append(("PhraseExpress, 32 px", Image.open(tmp).convert('RGBA'), (70, 30, 28)))
    PLACES[-1][1].load()
    os.remove(tmp)

sheet = Image.new("RGBA", (1180, 280), (24, 24, 24, 255))
d = ImageDraw.Draw(sheet)
d.text((20, 10), "Phrase X: icona in ogni posizione", fill=(255, 255, 255), font=font)
x = 20
for label, im, bg in PLACES:
    im = im.convert('RGBA')
    w = max(im.size[0] + 40, 170)
    sheet.paste(bg + (255,), (x, 55, x + w, 225))
    sheet.alpha_composite(im, (x + (w - im.size[0]) // 2, 140 - im.size[1] // 2))
    d.text((x, 232), label, fill=(170, 170, 170), font=small)
    x += w + 15
sheet.save(S + r'\icon-preview.png')
print(S + r'\icon-preview.png')
