#!/usr/bin/env python3
"""Render the BibleWolf app icon (PNG sizes + Windows .ico).

The wolf is the U+1F43A glyph from Noto Emoji (SIL OFL 1.1), the same font
egui bundles. Usage: make_icon.py <NotoEmoji-Regular.ttf> <out_dir>
"""
import sys
from PIL import Image, ImageDraw, ImageFont, ImageFilter

def render(font_path, size=1024):
    s = size
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    # Rounded-square tile with a vertical midnight gradient.
    grad = Image.new("RGBA", (s, s))
    top, bottom = (0x26, 0x2c, 0x3f), (0x0d, 0x0f, 0x15)
    gd = ImageDraw.Draw(grad)
    for y in range(s):
        t = y / (s - 1)
        gd.line([(0, y), (s, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,))
    mask = Image.new("L", (s, s), 0)
    inset, radius = int(s * 0.06), int(s * 0.22)
    ImageDraw.Draw(mask).rounded_rectangle([inset, inset, s - inset, s - inset], radius, fill=255)
    img.paste(grad, (0, 0), mask)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([inset, inset, s - inset, s - inset], radius, outline=(0xd9, 0xb2, 0x6a, 255), width=max(2, s // 64))

    # Gold wolf with a soft glow.
    font = ImageFont.truetype(font_path, int(s * 0.56))
    glyph = "\U0001F43A"
    bbox = d.textbbox((0, 0), glyph, font=font)
    w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
    pos = ((s - w) / 2 - bbox[0], (s - h) / 2 - bbox[1] + s * 0.01)
    glow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    ImageDraw.Draw(glow).text(pos, glyph, font=font, fill=(0xf0, 0xc9, 0x7c, 150))
    img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(s / 40)))
    ImageDraw.Draw(img).text(pos, glyph, font=font, fill=(0xf2, 0xcf, 0x86, 255))
    return img

if __name__ == "__main__":
    font, out = sys.argv[1], sys.argv[2]
    big = render(font, 1024)
    for size in (16, 32, 48, 64, 128, 256, 512, 1024):
        big.resize((size, size), Image.LANCZOS).save(f"{out}/biblewolf-{size}.png")
    big.save(f"{out}/biblewolf.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print("icons written to", out)
