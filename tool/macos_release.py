"""Sign and verify the macOS app for direct distribution on a disposable Mac."""

from __future__ import annotations

import argparse
import plistlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from macos_package import (
    ARCHITECTURES,
    DMG_FORMAT,
    artifact_name,
    digest,
    inspect_bundle,
    macho_files,
    run,
    sign_app,
)


def verify_signature(path: Path, fingerprint: str) -> None:
    run(["codesign", "--verify", "--strict", path])
    with tempfile.TemporaryDirectory() as directory:
        prefix = Path(directory) / "cert"
        run(["codesign", "--display", f"--extract-certificates={prefix}", path])
        if digest(Path(directory) / "cert0") != fingerprint:
            raise ValueError(f"Incorrect macOS signing certificate on {path.name}")


def check_source(app: Path, arch: str, tag: str, build_number: str) -> str:
    inspect_bundle(app, arch)
    with (app / "Contents/Info.plist").open("rb") as handle:
        info = plistlib.load(handle)
    version = tag.removeprefix("v")
    name = artifact_name(version, arch)
    if (
        tag != f"v{version}"
        or info.get("UsqueReleaseVersion") != version
        or info.get("CFBundleShortVersionString") != version.partition("-")[0]
        or not (build_number.isascii() and build_number.isdecimal() and build_number[0] != "0")
        or info.get("CFBundleVersion") != build_number
        or info.get("UsqueTestTunEnabled") is not False
        or info.get("UsqueAgentServiceIncluded") is not True
        or info.get("UsqueNetworkPrototypeIncluded") is not False
    ):
        raise ValueError("The release requires the exact tagged ordinary Agent build")
    return name


def create_image(
    app: Path,
    arch: str,
    tag: str,
    build_number: str,
    output: Path,
    keychain: Path,
    identity: str,
    fingerprint: str,
) -> None:
    name = check_source(app, arch, tag, build_number)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output) as directory:
        work = Path(directory)
        volume = work / "volume"
        volume.mkdir()
        bundled = volume / "Usque.app"
        shutil.copytree(app, bundled, symlinks=True)
        sign_app(bundled, identity, keychain)
        check_source(bundled, arch, tag, build_number)
        for path in [bundled, *macho_files(bundled)]:
            verify_signature(path, fingerprint)
        (volume / "Applications").symlink_to("/Applications")
        (volume / "READ ME.txt").write_text(
            "Usque for macOS. This package is project-signed and has no Apple "
            "notarization ticket. Verify the DMG fingerprint and GitHub build provenance "
            "using the release notes. System-wide TUN is disabled in this build.\n",
            encoding="utf-8",
        )
        image = work / name
        run(
            [
                "hdiutil",
                "create",
                "-srcfolder",
                volume,
                "-volname",
                "Usque",
                "-format",
                DMG_FORMAT,
                "-fs",
                "HFS+",
                image,
            ]
        )
        run(["codesign", "--sign", identity, "--keychain", keychain, "--timestamp=none", image])
        run(["hdiutil", "verify", image])
        verify_signature(image, fingerprint)
        image.replace(output / name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--arch", required=True, choices=ARCHITECTURES)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--build-number", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--keychain", required=True, type=Path)
    parser.add_argument("--identity", required=True)
    parser.add_argument("--fingerprint", required=True)
    args = parser.parse_args()
    try:
        create_image(
            args.app,
            args.arch,
            args.tag,
            args.build_number,
            args.output,
            args.keychain,
            args.identity,
            args.fingerprint,
        )
    except subprocess.CalledProcessError as error:
        print(error.output, file=sys.stderr)
        raise SystemExit(error.returncode) from error
