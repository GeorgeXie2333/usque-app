"""The native setup and uninstall windows must share the Flutter design tokens."""

from __future__ import annotations

import json
import re
import tempfile
import unittest
from pathlib import Path

import generate_windows_setup_tokens as generator

THEME = generator.ROOT / "apps/usque_gui/lib/core/usque_theme.dart"
MOTION = generator.ROOT / "apps/usque_gui/lib/core/usque_motion.dart"


def block(source: str, opening: str) -> str:
    start = source.index(opening) + len(opening)
    depth = 1
    for index in range(start, len(source)):
        depth += {"(": 1, ")": -1}.get(source[index], 0)
        if depth == 0:
            return source[start:index]
    raise ValueError(f"Unclosed Dart block: {opening}")


class FlutterTheme:
    def __init__(self) -> None:
        self.source = THEME.read_text(encoding="utf-8")
        self.motion = MOTION.read_text(encoding="utf-8")

    def color(self, expression: str) -> str:
        expression = expression.strip()
        literal = re.fullmatch(r"Color\(0xFF([0-9A-Fa-f]{6})\)", expression)
        if literal:
            return "#" + literal.group(1).upper()
        named = re.fullmatch(r"UsqueColors\.(\w+)", expression)
        if named:
            match = re.search(rf"static const Color {named.group(1)} = ([^;]+);", self.source)
            if match is None:
                raise KeyError(expression)
            value = match.group(1)
            return self.color(value if value.startswith("Color(") else f"UsqueColors.{value}")
        raise ValueError(f"Unsupported Dart color expression: {expression}")

    def field(self, opening: str, name: str) -> str:
        match = re.search(rf"\b{name}: ([^,\n]+),", block(self.source, opening))
        if match is None:
            raise KeyError(f"{opening} {name}")
        return match.group(1)

    def token_color(self, reference: str, theme: str) -> str:
        owner, name = reference.split(".")
        dark = theme == "dark"
        opening = {
            "UsqueTokens": "_dark = UsqueTokens(" if dark else "_light = UsqueTokens(",
            "ColorScheme": "_darkScheme = ColorScheme(" if dark else "_lightScheme = ColorScheme(",
        }[owner]
        return self.color(self.field(opening, name))

    def metric(self, reference: str) -> float:
        owner, name = reference.split(".")
        if owner == "UsqueRadii":
            match = re.search(rf"static const double {name} = ([\d.]+);", self.source)
        elif owner == "UsqueMotion":
            match = re.search(
                rf"static const Duration {name} = Duration\(milliseconds: (\d+)\);", self.motion
            )
        elif reference == "CheckboxThemeData.radius":
            match = re.search(
                r"BorderRadius\.circular\(([\d.]+)\)", block(self.source, "CheckboxThemeData(")
            )
        elif reference == "CheckboxThemeData.sideWidth":
            match = re.search(r"width: ([\d.]+)\)", block(self.source, "CheckboxThemeData("))
        elif reference == "FilledButtonThemeData.minimumHeight":
            match = re.search(
                r"minimumSize: const Size\(\d+, (\d+)\)",
                block(self.source, "FilledButtonThemeData("),
            )
        else:
            raise ValueError(f"Unsupported Flutter metric: {reference}")
        if match is None:
            raise KeyError(reference)
        return float(match.group(1))


class SetupTokenTests(unittest.TestCase):
    def setUp(self) -> None:
        self.model = generator.load()
        self.source = self.model["source"]

    def test_checked_in_projections_are_current(self) -> None:
        for language, contents in generator.render(self.model).items():
            self.assertEqual(generator.OUTPUTS[language].read_text(encoding="utf-8"), contents)

    def test_every_referenced_token_matches_the_flutter_theme(self) -> None:
        flutter = FlutterTheme()
        for name, entry in self.source["colors"].items():
            for theme in generator.THEMES:
                self.assertEqual(
                    entry[theme], flutter.token_color(entry["flutter"], theme), f"{name}/{theme}"
                )
        for name, entry in self.source["metrics"].items():
            if "flutter" in entry:
                self.assertEqual(entry["value"], flutter.metric(entry["flutter"]), name)
        for name, entry in self.source["ratios"].items():
            owner, field = entry["flutter"].split(".")
            self.assertEqual(owner, "UsqueTokens")
            for theme in generator.THEMES:
                opening = "_dark = UsqueTokens(" if theme == "dark" else "_light = UsqueTokens("
                self.assertEqual(
                    entry[theme], float(flutter.field(opening, field)), f"{name}/{theme}"
                )

    def test_established_brand_values_survive_the_shared_source(self) -> None:
        light, dark = self.model["palettes"]["light"], self.model["palettes"]["dark"]
        self.assertEqual(generator.colorref(light["primary"]), "0x000c50c2")
        self.assertEqual(generator.colorref(dark["primary"]), "0x005ca4ff")
        self.assertEqual(generator.colorref(light["primary_pressed"]), "0x000b48af")
        self.assertEqual(generator.colorref(dark["primary_pressed"]), "0x005394e6")
        self.assertEqual(generator.colorref(dark["on_primary"]), "0x00001844")

    def test_notices_follow_the_flutter_warning_banner_blend(self) -> None:
        light, dark = self.model["palettes"]["light"], self.model["palettes"]["dark"]
        self.assertEqual(
            light["success_soft"], generator.mix(light["success"], light["canvas"], 0.12)
        )
        self.assertEqual(dark["success_soft"], generator.mix(dark["success"], dark["canvas"], 0.16))
        self.assertEqual(
            light["caution_fill"], generator.mix(light["caution"], light["canvas"], 0.10)
        )
        self.assertEqual(
            light["caution_border"], generator.mix(light["caution"], light["canvas"], 0.30)
        )
        self.assertEqual(generator.mix((255, 0, 0), (0, 0, 255), 0.5), (128, 0, 128))

    def test_malformed_sources_are_rejected(self) -> None:
        source = json.loads(generator.SOURCE.read_text(encoding="utf-8"))
        cases = []
        bad_color = json.loads(json.dumps(source))
        bad_color["colors"]["canvas"]["light"] = "#f5f4f1"
        cases.append((bad_color, "upper case"))
        shadow = json.loads(json.dumps(source))
        shadow["derived"]["canvas"] = {"mix": ["ink", "canvas", 0.5]}
        cases.append((shadow, "shadows"))
        metric = json.loads(json.dumps(source))
        metric["metrics"]["focus_ring"]["value"] = 0
        cases.append((metric, "positive"))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tokens.json"
            for contents, message in cases:
                path.write_text(json.dumps(contents), encoding="utf-8")
                with self.assertRaisesRegex(ValueError, message):
                    generator.load(path)
            path.write_text('{"schema":1,"schema":1}', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Duplicate"):
                generator.load(path)


if __name__ == "__main__":
    unittest.main()
