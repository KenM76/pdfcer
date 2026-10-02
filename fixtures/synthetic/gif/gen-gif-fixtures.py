#!/usr/bin/env python3
"""Generate the GIF fixtures for `pdfcer_core::image_import::gif`.

Wholly synthetic (LEGAL.md section 5, category a): every pixel comes from a
formula below, and `crates/pdfcer-core/tests/image_gif.rs` re-derives the
same formulas as its oracle.

Written with Pillow's pure-Python GIF encoder on purpose. pdfcer's unit tests
build GIFs with weezl, the same crate pdfcer decodes with; a second,
independent encoder is what catches a bit-order or sub-block mistake the two
halves of one library would agree on.

Usage:  python fixtures/synthetic/gif/gen-gif-fixtures.py
"""

from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent

BLACK_WHITE = [0, 0, 0, 255, 255, 255]
FOUR = [200, 30, 30, 30, 160, 60, 40, 60, 220, 250, 250, 250]


def paletted(width, height, palette, pixel):
    img = Image.new("P", (width, height))
    img.putpalette(palette)
    img.putdata([pixel(x, y) for y in range(height) for x in range(width)])
    return img


def two_colour_transparent():
    # Checkerboard; index 1 (white) is the transparent index.
    img = paletted(8, 6, BLACK_WHITE, lambda x, y: (x + y) % 2)
    img.save(HERE / "two-colour-transparent.gif", transparency=1, optimize=False)


def interlaced():
    # Four horizontal bands one row tall, cycling, so a wrong pass order
    # moves a band. Pillow interlaces images 16 px or more in both axes.
    img = paletted(32, 20, FOUR, lambda x, y: (y + x // 8) % 4)
    img.save(HERE / "interlaced.gif", interlace=True, optimize=False)


def animated():
    # Three frames, each a single solid index, so the placed frame is
    # identifiable by colour alone.
    frames = [paletted(16, 16, FOUR, lambda x, y, i=i: i) for i in range(3)]
    frames[0].save(
        HERE / "animated-3-frames.gif",
        save_all=True,
        append_images=frames[1:],
        duration=100,
        loop=0,
        optimize=False,
        disposal=1,
    )


def truncated():
    # The interlaced fixture cut inside its first frame's image data.
    data = (HERE / "interlaced.gif").read_bytes()
    (HERE / "truncated.gif").write_bytes(data[: len(data) // 2])


if __name__ == "__main__":
    two_colour_transparent()
    interlaced()
    animated()
    truncated()
