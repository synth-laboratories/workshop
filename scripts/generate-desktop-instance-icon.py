#!/usr/bin/env python3
"""Create a deterministic numbered/lettered Synth development app icon."""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for candidate in (
        "/System/Library/Fonts/SFNSRounded.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    ):
        try:
            return ImageFont.truetype(candidate, size=size)
        except OSError:
            pass
    return ImageFont.load_default()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--png", type=Path, required=True)
    parser.add_argument("--icns", type=Path, required=True)
    parser.add_argument("--label", required=True)
    args = parser.parse_args()

    label = args.label.strip()[:2].upper()
    if not label:
        raise SystemExit("icon label is required")

    image = Image.open(args.source).convert("RGBA").resize((512, 512), Image.Resampling.LANCZOS)
    draw = ImageDraw.Draw(image)
    center = (392, 392)
    radius = 96
    box = (
        center[0] - radius,
        center[1] - radius,
        center[0] + radius,
        center[1] + radius,
    )
    draw.ellipse(box, fill=(18, 28, 52, 245), outline=(255, 255, 255, 255), width=12)

    badge_font = font(142 if len(label) == 1 else 104)
    text_box = draw.textbbox((0, 0), label, font=badge_font, stroke_width=2)
    width = text_box[2] - text_box[0]
    height = text_box[3] - text_box[1]
    origin = (center[0] - width / 2, center[1] - height / 2 - text_box[1])
    draw.text(origin, label, font=badge_font, fill="white", stroke_width=2, stroke_fill=(18, 28, 52))

    args.png.parent.mkdir(parents=True, exist_ok=True)
    args.icns.parent.mkdir(parents=True, exist_ok=True)
    image.save(args.png, format="PNG")
    image.save(args.icns, format="ICNS")


if __name__ == "__main__":
    main()
