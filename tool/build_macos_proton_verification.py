"""Build the pinned, sandbox-enabled standalone macOS Proton CEF component."""

from __future__ import annotations

import argparse
import json
import plistlib
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path

import macos_package

ROOT = Path(__file__).resolve().parents[1]
VERSION = "156.0.14+g4fa4ab5+chromium-156.0.8078.48"
PINS = {
    "arm64": {
        "platform": "macosarm64",
        "sha256": "ed994b907ad596ca609a0542a1e2b22aa97e93e98b8141fed4c24aef39b9ccf6",
        "official_sha1": "7359a9d293dd5c480bec3f18b762731b483b0e13",
    },
    "x86_64": {
        "platform": "macosx64",
        "sha256": "e584ade521802f8c2779d24996b32ddc183e8502743e5e3910b7247bb08f24f1",
        "official_sha1": "bc46c83878741fa2b23486940dacebabb294bf39",
    },
}
NAME = "UsqueProtonVerification"
IDENTIFIER = macos_package.IDENTIFIER + ".proton-verification"


def run(arguments: list[str | Path]) -> None:
    subprocess.run([str(value) for value in arguments], check=True)  # noqa: S603


def build(architecture: str, cef: Path, output: Path, *, verified_sdk=False) -> Path:
    pin = PINS[architecture]
    framework = cef / "Release/Chromium Embedded Framework.framework"
    macos_package.inspect_macho(framework / "Chromium Embedded Framework", architecture)
    header = (cef / "include/cef_version.h").read_text(encoding="utf-8")
    if f'#define CEF_VERSION "{VERSION}"' not in header:
        raise ValueError("CEF does not match the pinned reviewed API version")
    output.mkdir(parents=True, exist_ok=True)
    cmake = output / "cmake"
    run(
        [
            "cmake",
            "-S",
            ROOT / "apps/usque_gui/macos/ProtonVerification",
            "-B",
            cmake,
            "-G",
            "Ninja",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DUSE_SANDBOX=ON",
            f"-DCEF_ROOT={cef.resolve()}",
            f"-DPROJECT_ARCH={architecture}",
            f"-DCMAKE_OSX_ARCHITECTURES={architecture}",
            "-DCMAKE_OSX_DEPLOYMENT_TARGET=13.0",
        ]
    )
    run(["cmake", "--build", cmake, "--parallel", "4"])
    app = output / f"{NAME}.app"
    if app.exists():
        shutil.rmtree(app)
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Frameworks").mkdir()
    (contents / "Resources").mkdir()
    shutil.copy2(cmake / NAME, contents / "MacOS" / NAME)
    shutil.copytree(framework, contents / "Frameworks" / framework.name, symlinks=True)
    for source in ("LICENSE.txt", "CREDITS.html"):
        shutil.copy2(cef / source, contents / "Resources" / source)
    info = {
        "CFBundleExecutable": NAME,
        "CFBundleIdentifier": IDENTIFIER,
        "CFBundleName": "Proton verification",
        "CFBundlePackageType": "APPL",
        "CFBundleVersion": "1",
        "CFBundleShortVersionString": "0.1.0",
        "LSMinimumSystemVersion": "13.0",
        "LSUIElement": True,
        "NSHighResolutionCapable": True,
        "NSSupportsAutomaticGraphicsSwitching": True,
        "UsqueCEFSandboxRequired": True,
        "UsqueCEFVersion": VERSION,
    }
    (contents / "Info.plist").write_bytes(plistlib.dumps(info))
    for name, suffix in (
        ("", ""),
        (" (Alerts)", ".alerts"),
        (" (GPU)", ".gpu"),
        (" (Plugin)", ".plugin"),
        (" (Renderer)", ".renderer"),
    ):
        executable = NAME + " Helper" + name
        helper = contents / "Frameworks" / (executable + ".app") / "Contents"
        (helper / "MacOS").mkdir(parents=True)
        shutil.copy2(cmake / (NAME + "Helper"), helper / "MacOS" / executable)
        helper_info = {
            **info,
            "CFBundleExecutable": executable,
            "CFBundleIdentifier": IDENTIFIER + ".helper" + suffix,
            "CFBundleName": executable,
            "LSBackgroundOnly": True,
        }
        (helper / "Info.plist").write_bytes(plistlib.dumps(helper_info))
    macos_package._thin_and_sign(app, architecture)
    report = {
        "cef_version": VERSION,
        "architecture": architecture,
        "sandbox_compiled": True,
        "dependency": {
            **pin,
            "source": "https://cef-builds.spotifycdn.com/index.json"
            if verified_sdk
            else "custom_local_sdk_unverified",
            "verified_archive": verified_sdk,
        },
        "mach_o": [
            {"path": str(path.relative_to(app)), **macos_package.inspect_macho(path, architecture)}
            for path in macos_package.macho_files(app)
        ],
        "signed_production_launch": "not_run",
        "official_challenge": "not_run",
    }
    (output / "verification-build.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    return app


def main() -> None:
    import fcntl

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", required=True, choices=PINS)
    parser.add_argument("--cef-root", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    pin = PINS[args.arch]
    try:
        output = args.output or ROOT / "build/macos-proton-verification" / args.arch
        output.mkdir(parents=True, exist_ok=True)
        with (output / ".cef-build.lock").open("a") as lock:
            # Full application and standalone component builds can share this
            # architecture's generated SDK and app without racing replacement.
            fcntl.flock(lock, fcntl.LOCK_EX)
            cef = args.cef_root
            if cef is None:
                archive = ROOT / ".toolchains/cef" / (pin["platform"] + ".tar.bz2")
                if not archive.is_file() or macos_package.digest(archive) != pin["sha256"]:
                    raise ValueError(
                        "Missing or mismatched pinned CEF SDK archive; see the component README"
                    )
                if not hasattr(tarfile, "data_filter"):
                    raise ValueError(
                        "Safe CEF extraction requires tarfile.data_filter (Python 3.12+)"
                    )
                expected = f"cef_binary_{VERSION}_{pin['platform']}_minimal"
                with tempfile.TemporaryDirectory(prefix=".verified-cef-", dir=output) as directory:
                    temporary = Path(directory)
                    with tarfile.open(archive, "r:bz2") as source:
                        if any(
                            Path(member.name).parts[:1] != (expected,)
                            for member in source.getmembers()
                        ):
                            raise ValueError("Unexpected CEF SDK archive root")
                        source.extractall(temporary, filter="data")
                    macos_package.bundle_files(temporary / expected)
                    cef = output / "verified-cef-sdk"
                    if cef.exists():
                        shutil.rmtree(cef)
                    (temporary / expected).rename(cef)
            app = build(args.arch, cef, output, verified_sdk=args.cef_root is None)
        print(f"Built {app}; sandbox required; official challenge acceptance remains pending.")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"CEF verification build failed: {error}\n")


if __name__ == "__main__":
    main()
