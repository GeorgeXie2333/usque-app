"""Generate deterministic platform assets from the user-supplied Usque icon.

This intentionally uses Pillow rather than an image model: alpha edges, exact
dimensions, and repeatability matter for application packaging.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "branding" / "usque-app-icon.png"
FLUTTER = ROOT / "apps" / "usque_gui"
ORANGE = "#F48120"
INK = "#191C1E"


def resize(source: Image.Image, size: int) -> Image.Image:
    return source.resize((size, size), Image.Resampling.LANCZOS)


def save_android(source: Image.Image) -> None:
    resources = FLUTTER / "android" / "app" / "src" / "main" / "res"
    legacy_sizes = {
        "mipmap-mdpi": 48,
        "mipmap-hdpi": 72,
        "mipmap-xhdpi": 96,
        "mipmap-xxhdpi": 144,
        "mipmap-xxxhdpi": 192,
    }
    foreground_sizes = {
        "mipmap-mdpi": 108,
        "mipmap-hdpi": 162,
        "mipmap-xhdpi": 216,
        "mipmap-xxhdpi": 324,
        "mipmap-xxxhdpi": 432,
    }
    for folder, size in legacy_sizes.items():
        destination = resources / folder
        destination.mkdir(parents=True, exist_ok=True)
        resize(source, size).save(destination / "ic_launcher.png", optimize=True)

    for folder, size in foreground_sizes.items():
        destination = resources / folder
        canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        artwork_size = round(size * 0.72)
        artwork = resize(source, artwork_size)
        offset = (size - artwork_size) // 2
        canvas.alpha_composite(artwork, (offset, offset))
        canvas.save(destination / "ic_launcher_foreground.png", optimize=True)

    banner_dir = resources / "drawable-xhdpi"
    banner_dir.mkdir(parents=True, exist_ok=True)
    banner = Image.new("RGB", (320, 180), "white")
    draw = ImageDraw.Draw(banner)
    draw.ellipse((-60, -90, 180, 150), fill="#FFF0E3")
    icon = resize(source, 112)
    banner.paste(icon, (28, 34), icon)
    title_font = ImageFont.truetype(
        r"C:\Windows\Fonts\segoeuib.ttf",
        38,
    )
    subtitle_font = ImageFont.truetype(
        r"C:\Windows\Fonts\segoeui.ttf",
        14,
    )
    draw.text((158, 56), "Usque", fill=ORANGE, font=title_font)
    draw.text((160, 105), "Native WARP client", fill=INK, font=subtitle_font)
    banner.save(banner_dir / "tv_banner.png", optimize=True)


def save_macos(source: Image.Image) -> None:
    destination = (
        FLUTTER
        / "macos"
        / "Runner"
        / "Assets.xcassets"
        / "AppIcon.appiconset"
    )
    destination.mkdir(parents=True, exist_ok=True)
    for size in (16, 32, 64, 128, 256, 512, 1024):
        resize(source, size).save(
            destination / f"app_icon_{size}.png",
            optimize=True,
        )


def save_windows(source: Image.Image) -> None:
    windows_icon = (
        FLUTTER / "windows" / "runner" / "resources" / "app_icon.ico"
    )
    windows_icon.parent.mkdir(parents=True, exist_ok=True)
    source.save(
        windows_icon,
        format="ICO",
        sizes=[(size, size) for size in (16, 24, 32, 48, 64, 128, 256)],
    )


def save_distribution_icons(source: Image.Image) -> None:
    branding = ROOT / "assets" / "branding"
    source.save(
        branding / "usque-app-icon.ico",
        format="ICO",
        sizes=[(size, size) for size in (16, 24, 32, 48, 64, 128, 256)],
    )
    source.save(
        branding / "usque-app-icon.icns",
        format="ICNS",
        sizes=[(size, size) for size in (16, 32, 64, 128, 256, 512, 1024)],
    )


def save_flutter_ui_icon(source: Image.Image) -> None:
    """Write a compact texture used only by Flutter's in-app brand chrome."""
    destination = FLUTTER / "assets" / "branding" / "usque-ui-icon.png"
    destination.parent.mkdir(parents=True, exist_ok=True)
    resize(source, 256).save(destination, optimize=True)


def save_readme_banner(source: Image.Image) -> None:
    width, height = 1600, 500
    banner = Image.new("RGB", (width, height), "white")
    draw = ImageDraw.Draw(banner)
    draw.ellipse((-220, -330, 640, 530), fill="#FFF3E8")
    draw.ellipse((1320, 250, 1770, 700), fill="#FFF8F2")
    icon = resize(source, 330)
    banner.paste(icon, (120, 85), icon)
    title_font = ImageFont.truetype(
        r"C:\Windows\Fonts\segoeuib.ttf",
        112,
    )
    subtitle_font = ImageFont.truetype(
        r"C:\Windows\Fonts\segoeui.ttf",
        39,
    )
    detail_font = ImageFont.truetype(
        r"C:\Windows\Fonts\segoeui.ttf",
        28,
    )
    draw.text((520, 115), "Usque", fill=ORANGE, font=title_font)
    draw.text(
        (528, 265),
        "Unofficial client compatible with Cloudflare WARP",
        fill=INK,
        font=subtitle_font,
    )
    draw.text(
        (530, 333),
        "Native Flutter interface · Rust networking core",
        fill="#66615E",
        font=detail_font,
    )
    banner.save(
        ROOT / "assets" / "branding" / "usque-readme-banner.png",
        optimize=True,
    )


def main() -> None:
    source = Image.open(SOURCE).convert("RGBA")
    if source.size[0] != source.size[1]:
        raise ValueError(f"App icon must be square, got {source.size}")
    alpha = source.getchannel("A")
    if alpha.getextrema()[0] == 255:
        raise ValueError("App icon has no transparent pixels")

    save_android(source)
    save_macos(source)
    save_windows(source)
    save_distribution_icons(source)
    save_flutter_ui_icon(source)
    save_readme_banner(source)


if __name__ == "__main__":
    main()
