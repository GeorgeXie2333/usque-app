"""Generate deterministic platform assets from the user-supplied Usque icon.

This intentionally uses Pillow rather than an image model: alpha edges, exact
dimensions, and repeatability matter for application packaging.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont, ImageMath

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "branding" / "usque-app-icon.png"
FLUTTER = ROOT / "apps" / "usque_gui"
ORANGE = "#C2500C"
INK = "#191C1E"
LIGHT_FILL = (194, 80, 12)
LIGHT_LINE = (245, 244, 241)
DARK_FILL = (255, 164, 92)
DARK_LINE = (68, 24, 0)


def line_mask(source: Image.Image) -> Image.Image:
    """Recover line coverage, including antialiasing, from the two source colours."""
    red, green, blue, alpha = source.split()
    delta = tuple(line - fill for line, fill in zip(LIGHT_LINE, LIGHT_FILL, strict=True))
    denominator = sum(channel * channel for channel in delta)
    coverage = ImageMath.lambda_eval(
        lambda channels: (
            (
                (channels["r"] - LIGHT_FILL[0]) * delta[0]
                + (channels["g"] - LIGHT_FILL[1]) * delta[1]
                + (channels["b"] - LIGHT_FILL[2]) * delta[2]
            )
            * (255.0 / denominator)
        ),
        r=red.convert("F"),
        g=green.convert("F"),
        b=blue.convert("F"),
    ).convert("L")
    # The line artwork is entirely inside the opaque disk. Its exterior edge
    # has quantized RGB values at low alpha; those are fill coverage, not line
    # coverage, and must not become a faint ring in monochrome assets.
    opaque = alpha.point(lambda value: 255 if value == 255 else 0)
    return ImageMath.lambda_eval(
        lambda channels: channels["coverage"] * channels["alpha"] / 255.0 + 0.5,
        coverage=coverage.convert("F"),
        alpha=opaque.convert("F"),
    ).convert("L")


def dark_logo(source: Image.Image, mask: Image.Image) -> Image.Image:
    """Recolour before resizing; keep the source's exterior alpha unchanged."""
    # Exterior pixels belong to the fill, not the line. Undo alpha only where
    # coverage exists so partially transparent outer edges cannot acquire a halo.
    coverage = ImageMath.lambda_eval(
        lambda channels: channels["mask"] * 255.0 / channels["alpha"],
        mask=mask.convert("F"),
        alpha=source.getchannel("A").point(lambda value: max(1, value)).convert("F"),
    ).convert("L")
    channels = [
        coverage.point(
            lambda value, fill=fill, line=line: round(fill + (line - fill) * value / 255)
        )
        for fill, line in zip(DARK_FILL, DARK_LINE, strict=True)
    ]
    return Image.merge("RGBA", (*channels, source.getchannel("A")))


def monochrome_logo(mask: Image.Image) -> Image.Image:
    """The U/star artwork remains visible after Android applies a system tint."""
    result = Image.new("RGBA", mask.size, "white")
    result.putalpha(mask)
    return result


def fit_artwork(
    source: Image.Image,
    canvas_size: int,
    artwork_size: int,
    *,
    reference_bounds: tuple[int, int, int, int] | None = None,
) -> Image.Image:
    bounds = reference_bounds or source.getchannel("A").getbbox()
    if bounds is None:
        raise ValueError("Brand artwork is empty")
    artwork = source.crop(bounds)
    artwork.thumbnail((artwork_size, artwork_size), Image.Resampling.LANCZOS)
    occupied = artwork.getchannel("A").getbbox()
    if occupied is None:
        raise ValueError("Brand artwork disappeared when resized")
    # Keep the lines in their original position when the disk moves into the
    # adaptive background. Other assets still centre their occupied artwork.
    if reference_bounds is None:
        artwork = artwork.crop(occupied)
    canvas = Image.new("RGBA", (canvas_size, canvas_size))
    canvas.alpha_composite(
        artwork, ((canvas_size - artwork.width) // 2, (canvas_size - artwork.height) // 2)
    )
    return canvas


def resize(source: Image.Image, size: int) -> Image.Image:
    return source.resize((size, size), Image.Resampling.LANCZOS)


def save_android(source: Image.Image, monochrome: Image.Image) -> None:
    resources = FLUTTER / "android" / "app" / "src" / "main" / "res"
    # Android minSdk 26 uses adaptive launcher icons; only their foreground
    # layers need density-specific bitmap variants.
    foreground_sizes = {
        "mipmap-mdpi": 108,
        "mipmap-hdpi": 162,
        "mipmap-xhdpi": 216,
        "mipmap-xxhdpi": 324,
        "mipmap-xxxhdpi": 432,
    }
    # Android composites adaptive icons over black. Use an opaque brand-colour
    # background in XML and derive a line-only foreground from the same master.
    disk_bounds = source.getchannel("A").getbbox()
    for folder, size in foreground_sizes.items():
        destination = resources / folder
        destination.mkdir(parents=True, exist_ok=True)
        artwork_size = round(size * 66 / 108)
        line_layer = fit_artwork(monochrome, size, artwork_size, reference_bounds=disk_bounds)
        # Resize coverage independently so premultiplied RGBA interpolation
        # cannot shift the line colour at antialiased edges.
        foreground = Image.new("RGBA", line_layer.size, LIGHT_LINE)
        foreground.putalpha(line_layer.getchannel("A"))
        foreground.save(destination / "ic_launcher_foreground.png", optimize=True)
        fit_artwork(monochrome, size, artwork_size).save(
            destination / "ic_launcher_monochrome.png", optimize=True
        )
        notification_dir = resources / folder.replace("mipmap-", "drawable-")
        notification_dir.mkdir(parents=True, exist_ok=True)
        notification_size = round(size * 24 / 108)
        fit_artwork(monochrome, notification_size, round(notification_size * 20 / 24)).save(
            notification_dir / "ic_stat_usque.png", optimize=True
        )

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
    # Notification bitmaps introduce all density folders. Keep the TV banner
    # complete in those folders too, rather than suppressing IconDensities.
    for folder, size in foreground_sizes.items():
        destination = resources / folder.replace("mipmap-", "drawable-")
        banner.resize(
            (round(320 * size / 216), round(180 * size / 216)), Image.Resampling.LANCZOS
        ).save(destination / "tv_banner.png", optimize=True)


def save_macos(source: Image.Image) -> None:
    destination = FLUTTER / "macos" / "Runner" / "Assets.xcassets" / "AppIcon.appiconset"
    destination.mkdir(parents=True, exist_ok=True)
    for size in (16, 32, 64, 128, 256, 512, 1024):
        resize(source, size).save(
            destination / f"app_icon_{size}.png",
            optimize=True,
        )


def save_windows(source: Image.Image) -> None:
    windows_icon = FLUTTER / "windows" / "runner" / "resources" / "app_icon.ico"
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


def save_flutter_ui_icon(source: Image.Image, dark: Image.Image) -> None:
    """Write a compact texture used only by Flutter's in-app brand chrome."""
    destination = FLUTTER / "assets" / "branding" / "usque-ui-icon.png"
    destination.parent.mkdir(parents=True, exist_ok=True)
    resize(source, 256).save(destination, optimize=True)
    resize(dark, 256).save(destination.with_name("usque-ui-icon-dark.png"), optimize=True)


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
        34,
    )
    detail_font = ImageFont.truetype(
        r"C:\Windows\Fonts\segoeui.ttf",
        28,
    )
    draw.text((520, 115), "Usque", fill=ORANGE, font=title_font)
    draw.text(
        (528, 265),
        "Unofficial client compatible with Cloudflare® WARP® services",
        fill=INK,
        font=subtitle_font,
    )
    draw.text(
        (530, 333),
        "Native Flutter interface · Rust networking core",
        fill="#66615E",
        font=detail_font,
    )
    attribution_font = ImageFont.truetype(r"C:\Windows\Fonts\segoeui.ttf", 17)
    draw.text(
        (530, 400),
        "Cloudflare and WARP are trademarks and/or registered trademarks of",
        fill="#66615E",
        font=attribution_font,
    )
    draw.text(
        (530, 428),
        "Cloudflare, Inc. in the United States and other jurisdictions.",
        fill="#66615E",
        font=attribution_font,
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

    mask = line_mask(source)
    save_android(source, monochrome_logo(mask))
    save_macos(source)
    save_windows(source)
    save_distribution_icons(source)
    save_flutter_ui_icon(source, dark_logo(source, mask))
    save_readme_banner(source)


if __name__ == "__main__":
    main()
