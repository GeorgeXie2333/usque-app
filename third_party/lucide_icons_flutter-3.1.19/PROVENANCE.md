# Regular Lucide font package

Source: [lucide_icons_flutter 3.1.19](https://pub.dev/packages/lucide_icons_flutter/versions/3.1.19),
from [the upstream repository](https://github.com/vqh2602/lucide-flutter-main).
The hosted package archive SHA-256 from the original app lockfile is
`3d06144ccc67c29fd080704ee59b6e1927f67654a848443eceb130b2e425e080`.

## Original file hashes

| File | SHA-256 |
| --- | --- |
| `pubspec.yaml` | `87c7a044524d6981fbb3605e69d9c0587809c55998d84b3850d13509e17dc063` |
| `lib/lucide_icons.dart` | `292652dd90c3ded7e8d2cb81475c795dd439b7afa627b9b2a73e648011d7f893` |
| `assets/lucide.ttf` | `b27152b2215731ec9a65a3b351b45873f912bf5d15a64e058aac4136699b804e` |
| `LICENSE` | `e314b096fc1cfa025b5e43cc5774fadbaec8f5f4fd4f0c415fe17fbcc1410dd5` |

## Narrow local changes

- Keep the original regular `assets/lucide.ttf` and MIT license byte-for-byte.
- Retain all 4124 `IconData` constants whose font family is exactly
  `Lucide`, including directional variants. Preserve every constant name,
  code point, package, and `matchTextDirection` value.
- Omit constants and font declarations for `Lucide100` through `Lucide600`.
  The app uses none of these variants; referencing one now fails at compile time.
- Omit embedded SVG documentation previews, the upstream icon gallery,
  development dependencies, and unused package files. These are source-only
  omissions, not additional claims of installed-size savings.
- Retain `@staticIconProvider`, the package name and import path so Flutter can
  subset the ordinary font without changing application imports or rendering.

To update, review a new upstream package and repeat this family-based
selection. Do not select constants by numeric name suffix: numbers can be
part of an ordinary icon name. Recheck all app and test references, font and
constant equivalence, and the existing exact-pixel Windows golden suite.
