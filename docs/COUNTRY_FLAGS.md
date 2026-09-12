# Country and region flags

The Flutter application bundles the complete [Flagpedia 80px PNG package](https://flagpedia.net/download/images)
from [FlagCDN](https://flagcdn.com/w80.zip). Flag images are identified as public
domain in the [provider's terms](https://flagpedia.net/terms). Attribution appears
in the application's existing third-party license page.

The original PNG files live in `apps/usque_gui/assets/flags/w80/`. The adjacent
`manifest.json` records the source, retrieval time, archive SHA-256, and each
file's dimensions and SHA-256. The matching Dart resource index is
`apps/usque_gui/lib/core/country_flags.dart`. Update the PNG bundle, manifest
and index together when deliberately refreshing assets for an application
release; ordinary builds and application startup never download flags.

`CountryFlag` normalizes a country code and looks it up in that index. Its
reserved box is 24 by 18 logical pixels, or 32 by 24 in directional navigation.
Images retain their original aspect ratios, including square and nonrectangular
flags. Unsupported codes and failed asset loads use a globe in the same box.
Adjacent text supplies accessible names; flags do not add focus targets.

VPN Gate filters, nodes and selection summaries use catalog country codes.
Home's exit region and Location use the measured exit country code independently
of the selected node. Geo direct settings use the existing country catalog.
Disabled VPN Gate server rows also mute their flags, without changing the
currently connected node or allowing a draft selection.

IP and location probes retain their existing timing, retry and cancellation
behavior. They no longer download or cache SVG flags. The legacy flag fields
remain in IPC and Android snapshots for wire compatibility; the UI ignores
them. Existing SVG cache files remain available to the existing user-initiated
cache cleanup operation, with no startup migration.

When updating assets, verify every SHA-256, decode every PNG, check that each
source width is 80 pixels, and keep full coverage of the shipped ISO country
list. Run the Flutter tests and review affected Windows golden images,
including square, long and nonrectangular flags in both themes.
