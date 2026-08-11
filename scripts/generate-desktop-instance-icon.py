#!/usr/bin/env python3
"""Create a deterministic, release-marked Synth development app icon."""

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
    parser.add_argument("--release-label", required=True)
    parser.add_argument("--instance-label", required=True)
    args = parser.parse_args()

    release_label = args.release_label.strip()[:6]
    instance_label = args.instance_label.strip()[:2].upper()
    if not release_label or not instance_label:
        raise SystemExit("release and instance labels are required")

    image = Image.open(args.source).convert("RGBA").resize((512, 512), Image.Resampling.LANCZOS)
    draw = ImageDraw.Draw(image)

    # The release line is the primary identity. It remains readable in Finder,
    # the Dock, screenshots, and CUA captures; the circular badge identifies
    # the exact concurrent instance.
    ribbon = (54, 34, 458, 142)
    draw.rounded_rectangle(
        ribbon,
        radius=34,
        fill=(18, 28, 52, 245),
        outline=(255, 255, 255, 255),
        width=9,
    )
    release_font = font(78)
    release_box = draw.textbbox((0, 0), release_label, font=release_font, stroke_width=2)
    release_width = release_box[2] - release_box[0]
    release_height = release_box[3] - release_box[1]
    release_origin = (
        256 - release_width / 2,
        88 - release_height / 2 - release_box[1],
    )
    draw.text(
        release_origin,
        release_label,
        font=release_font,
        fill="white",
        stroke_width=2,
        stroke_fill=(18, 28, 52),
    )

    center = (392, 392)
    radius = 96
    box = (
        center[0] - radius,
        center[1] - radius,
        center[0] + radius,
        center[1] + radius,
    )
    draw.ellipse(box, fill=(18, 28, 52, 245), outline=(255, 255, 255, 255), width=12)

    badge_font = font(142 if len(instance_label) == 1 else 104)
    text_box = draw.textbbox((0, 0), instance_label, font=badge_font, stroke_width=2)
    width = text_box[2] - text_box[0]
    height = text_box[3] - text_box[1]
    origin = (center[0] - width / 2, center[1] - height / 2 - text_box[1])
    draw.text(origin, instance_label, font=badge_font, fill="white", stroke_width=2, stroke_fill=(18, 28, 52))

    args.png.parent.mkdir(parents=True, exist_ok=True)
    args.icns.parent.mkdir(parents=True, exist_ok=True)
    image.save(args.png, format="PNG")
    image.save(args.icns, format="ICNS")


if __name__ == "__main__":
    main()
