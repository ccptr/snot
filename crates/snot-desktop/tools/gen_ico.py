#!/usr/bin/env python3
"""Renders the app icon into the Windows .ico that build.rs links into
snot.exe. Run from the repository root after changing the SVG:

    python3 crates/snot-desktop/tools/gen_ico.py

Needs rsvg-convert. Every size is stored as PNG, which Windows reads since
Vista; ImageMagick would store them as bitmaps, twenty times the size.
"""

import pathlib
import struct
import subprocess

ASSETS = pathlib.Path(__file__).resolve().parents[1] / "assets"
SRC = ASSETS / "app-icon.svg"
OUT = ASSETS / "windows/snot.ico"
# Explorer's and the taskbar's sizes at 100 to 250% scaling.
SIZES = [16, 20, 24, 32, 40, 48, 64, 256]


def render(size):
    return subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), str(SRC)],
        check=True, capture_output=True).stdout


def main():
    images = [render(s) for s in SIZES]
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = len(out) + 16 * len(images)
    for size, png in zip(SIZES, images):
        # A width or height of 0 means 256.
        out += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32,
                           len(png), offset)
        offset += len(png)
    OUT.write_bytes(out + b"".join(images))


if __name__ == "__main__":
    main()
