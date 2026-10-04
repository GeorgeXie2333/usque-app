"""Validate and embed the shared Windows setup copy without runtime files."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "packaging/windows/setup/strings.json"


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate localization key: {key}")
        result[key] = value
    return result


def load_catalog(path: Path) -> dict[str, dict[str, str]]:
    catalog = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    if not isinstance(catalog, dict) or not isinstance(catalog.get("en-US"), dict):
        raise ValueError("The setup catalog must contain en-US")
    keys = set(catalog["en-US"])
    if "language_name" not in keys:
        raise ValueError("Each language needs its own display name")
    for culture, strings in catalog.items():
        if not re.fullmatch(r"[a-z]{2}-[A-Z]{2}", culture):
            raise ValueError(f"Invalid culture: {culture}")
        if not isinstance(strings, dict) or set(strings) != keys:
            raise ValueError(f"Incomplete setup localization: {culture}")
        for key, value in strings.items():
            if not re.fullmatch(r"[a-z][a-z0-9_]*", key):
                raise ValueError(f"Invalid setup key: {key}")
            if not isinstance(value, str) or not value.strip() or "\x00" in value:
                raise ValueError(f"Empty or invalid setup copy: {culture}/{key}")
            expected = sorted(re.findall(r"\{[a-z_]+\}", catalog["en-US"][key]))
            actual = sorted(re.findall(r"\{[a-z_]+\}", value))
            if actual != expected:
                raise ValueError(f"Placeholder mismatch: {culture}/{key}")
    return catalog


def cpp_text(value: str) -> str:
    return "L" + json.dumps(value, ensure_ascii=False)


def render_cpp(catalog: dict[str, dict[str, str]]) -> str:
    lines = [
        "// Generated from the shared setup catalog. Do not edit.",
        "#pragma once",
        "#include <string_view>",
        "namespace usque::setup {",
        "struct Language { std::wstring_view code; std::wstring_view name; };",
        "inline constexpr Language kLanguages[] = {",
    ]
    for culture, strings in catalog.items():
        lines.append(f"  {{{cpp_text(culture)}, {cpp_text(strings['language_name'])}}},")
    lines.extend(
        [
            "};",
            "struct Translation { std::wstring_view culture; std::string_view key;",
            "  std::wstring_view value; };",
            "inline constexpr Translation kTranslations[] = {",
        ]
    )
    for culture, strings in catalog.items():
        for key, value in strings.items():
            lines.append(f'  {{{cpp_text(culture)}, "{key}", {cpp_text(value)}}},')
    lines.extend(
        [
            "};",
            "inline std::wstring_view Lookup(std::wstring_view culture,",
            "                                std::string_view key) {",
            "  std::wstring_view fallback;",
            "  for (const auto& text : kTranslations) {",
            "    if (text.key != key) continue;",
            "    if (text.culture == culture) return text.value;",
            '    if (text.culture == L"en-US") fallback = text.value;',
            "  }",
            "  return fallback;",
            "}",
            "}  // namespace usque::setup",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=SOURCE)
    parser.add_argument("--cpp", type=Path)
    arguments = parser.parse_args()
    catalog = load_catalog(arguments.source)
    if arguments.cpp:
        arguments.cpp.parent.mkdir(parents=True, exist_ok=True)
        arguments.cpp.write_text(render_cpp(catalog), encoding="utf-8", newline="\n")
    print(f"Validated {len(catalog)} setup languages")


if __name__ == "__main__":
    main()
