#!/usr/bin/env python3
"""Build the glyph-free font used to simulate unavailable golden font families."""

import argparse
import hashlib
from pathlib import Path

import fontTools
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", type=Path, default=Path(__file__).with_name("MissingFamily.ttf")
    )
    args = parser.parse_args()
    if fontTools.__version__ != "4.60.1":
        raise SystemExit("Regeneration requires FontTools 4.60.1.")
    builder = FontBuilder(1024, isTTF=True)
    builder.setupGlyphOrder([".notdef"])
    builder.setupCharacterMap({})
    builder.setupGlyf({".notdef": TTGlyphPen(None).glyph()})
    builder.setupHorizontalMetrics({".notdef": (0, 0)})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable(
        {
            "familyName": "Usque Golden Missing Family",
            "styleName": "Regular",
            "uniqueFontIdentifier": "UsqueGoldenMissingFamily-1.0",
            "fullName": "Usque Golden Missing Family Regular",
            "psName": "UsqueGoldenMissingFamily-Regular",
            "version": "Version 1.0",
            "description": "Glyph-free placeholder used only in Linux golden tests.",
        }
    )
    builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    builder.setupPost()
    builder.setupMaxp()
    builder.font["head"].created = 2082844800
    builder.font["head"].modified = 2082844800
    builder.font.recalcTimestamp = False
    builder.save(args.output)
    print(f"{hashlib.sha256(args.output.read_bytes()).hexdigest()}  {args.output.name}")


if __name__ == "__main__":
    main()
