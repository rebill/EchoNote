#!/usr/bin/env python3
"""Draw the EchoNote icon sources: icon-source.png (1024 px app icon) and tray-template.png
(monochrome menu-bar glyph for macOS, alpha only).

    python3 app/src-tauri/icons/make_source.py
    cd app && pnpm tauri icon src-tauri/icons/icon-source.png
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
# Relative bar heights of the waveform glyph.
BARS = [0.34, 0.62, 1.0, 0.74, 0.46]


def waveform(draw: ImageDraw.ImageDraw, box: tuple[int, int, int, int], fill) -> None:
    left, top, right, bottom = box
    width = right - left
    bar = width / (2 * len(BARS) - 1)
    middle = (top + bottom) / 2
    for i, height in enumerate(BARS):
        x = left + 2 * i * bar
        half = (bottom - top) * height / 2
        draw.rounded_rectangle(
            (x, middle - half, x + bar, middle + half), radius=bar / 2, fill=fill
        )


def app_icon() -> Image.Image:
    size, scale = 1024, 4
    big = size * scale
    gradient = Image.new("RGBA", (big, big))
    top, bottom = (24, 181, 165), (59, 108, 246)
    pixels = gradient.load()
    for y in range(big):
        for x in range(0, big, 8):
            t = (x + y) / (2 * big)
            color = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,)
            for dx in range(8):
                pixels[x + dx, y] = color
    # macOS icon grid: an 824 px body centred in 1024.
    mask = Image.new("L", (big, big), 0)
    margin = 100 * scale
    ImageDraw.Draw(mask).rounded_rectangle(
        (margin, margin, big - margin, big - margin), radius=185 * scale, fill=255
    )
    icon = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    icon.paste(gradient, (0, 0), mask)
    waveform(
        ImageDraw.Draw(icon),
        (330 * scale, 330 * scale, 694 * scale, 694 * scale),
        (255, 255, 255, 255),
    )
    return icon.resize((size, size), Image.LANCZOS)


def tray_template() -> Image.Image:
    size, scale = 44, 8
    big = size * scale
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    waveform(ImageDraw.Draw(image), (6 * scale, 6 * scale, 38 * scale, 38 * scale), (0, 0, 0, 255))
    return image.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    app_icon().save(HERE / "icon-source.png")
    tray_template().save(HERE / "tray-template.png")
    print("wrote icon-source.png and tray-template.png")
