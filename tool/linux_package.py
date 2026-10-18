"""Build and inspect Linux packages without installing or starting services."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shlex
import shutil
import stat
import struct
import subprocess
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORMATS = ("AppImage", "deb", "rpm")
ARCHITECTURES = {"amd64": (62, "x86_64", "x64"), "arm64": (183, "aarch64", "arm64")}
SERVICE_FILES = (
    "usr/lib/usque/usque-agent",
    "usr/lib/usque/usque-engine",
    "usr/lib/usque/usque-update",
    "usr/lib/systemd/system/usque-agent.service",
    "usr/share/polkit-1/actions/io.github.georgexie2333.usque.policy",
)
VERSION = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
GTK_SCHEMAS = tuple(
    f"org.gtk.Settings.{name}.gschema.xml"
    for name in ("ColorChooser", "Debug", "EmojiChooser", "FileChooser")
)
GTK_RESOURCE_DIRECTORIES = (
    "gtk-3.0",
    "themes/Default/gtk-3.0",
    "themes/Emacs/gtk-3.0",
    "icons/Adwaita",
)
DEBIAN_PACKAGE = re.compile(r"[a-z0-9][a-z0-9+.-]+(?::[a-z0-9][a-z0-9-]*)?")
# Some distribution notices spell these versions with .0, while base-files
# installs GPL-2 and GPL-3. Keep the complete text at the notice's reference.
COMMON_LICENSE_ALIASES = {"GPL-2.0": "GPL-2", "GPL-3.0": "GPL-3"}


def run(arguments: list[str | Path], *, cwd: Path | None = None) -> str:
    # Fixed executable/argument vectors; no shell expansion or secret logging.
    return subprocess.check_output(  # noqa: S603
        [str(value) for value in arguments], cwd=cwd, text=True, stderr=subprocess.STDOUT
    )


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as handle:
        for data in iter(lambda: handle.read(1024 * 1024), b""):
            result.update(data)
    return result.hexdigest()


def artifact_name(version: str, architecture: str, package_format: str) -> str:
    if (
        not VERSION.fullmatch(version)
        or architecture not in ARCHITECTURES
        or package_format not in FORMATS
    ):
        raise ValueError("Invalid Linux package identity")
    return f"usque-v{version}-linux-{architecture}.{package_format}"


def inspect_elf(path: Path, architecture: str) -> dict[str, object]:
    with path.open("rb") as handle:
        header = handle.read(64)
    if len(header) != 64 or header[:6] != b"\x7fELF\x02\x01":
        raise ValueError(f"Not a 64-bit little-endian ELF: {path.name}")
    if struct.unpack_from("<H", header, 18)[0] != ARCHITECTURES[architecture][0]:
        raise ValueError(f"Wrong ELF architecture: {path.name}")
    dynamic = run(["readelf", "--dynamic", "--wide", path])
    versions = run(["readelf", "--version-info", "--wide", path])
    notes = run(["readelf", "--notes", "--wide", path])
    if architecture == "amd64" and re.search(r"x86-64-v[234]", notes):
        raise ValueError(f"ELF requires a newer x86-64 ISA: {path.name}")
    for family, limit in (("GLIBC", (2, 35)), ("GLIBCXX", (3, 4, 30)), ("CXXABI", (1, 3, 13))):
        required = {
            tuple(map(int, value.split(".")))
            for value in re.findall(rf"\b{family}_([0-9.]+)", versions)
        }
        if any(value > limit for value in required):
            raise ValueError(f"ELF exceeds Ubuntu 22.04 {family} baseline: {path.name}")
    if "GLIBC_PRIVATE" in versions:
        raise ValueError(f"ELF depends on private glibc symbols: {path.name}")
    return {"path": path.name, "needed": re.findall(r"\(NEEDED\).*\[([^]]+)\]", dynamic)}


def inspect_bundle(directory: Path, architecture: str) -> list[dict[str, object]]:
    required = [
        directory / name
        for name in (
            "usque",
            "usque-engine",
            "usque-agent",
            "usque-update",
            "lib/libflutter_linux_gtk.so",
            "lib/libapp.so",
            "data/icudtl.dat",
        )
    ]
    if not all(path.is_file() and not path.is_symlink() for path in required):
        raise ValueError("Linux release bundle is incomplete")
    if (directory / "usque_ui_preview").exists() or (
        directory / "data/flutter_assets/kernel_blob.bin"
    ).exists():
        raise ValueError("Preview/debug content must not enter a production package")
    reports = []
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            if not path.resolve().is_relative_to(directory.resolve()):
                raise ValueError("Bundle contains an escaping symlink")
        elif path.is_file():
            with path.open("rb") as handle:
                magic = handle.read(4)
            if magic == b"\x7fELF":
                reports.append(inspect_elf(path, architecture))
    return reports


def bundle_libraries(directory: Path, architecture: str) -> dict[Path, Path]:
    """Resolve ELF dependencies without executing ldd or any payload binary."""
    cache = run(["/sbin/ldconfig", "-p"])
    libraries = {}
    for name, path in re.findall(r"^\s+(\S+) \([^\n]+\) => (\S+)$", cache, re.MULTILINE):
        libraries.setdefault(name, Path(path))
    # glibc/loader and graphics drivers must come from the host. Everything else
    # in the ordinary ELF closure is bundled, including GTK on KDE desktops.
    host = {
        "libc.so.6",
        "libm.so.6",
        "libdl.so.2",
        "libpthread.so.0",
        "librt.so.1",
        "libresolv.so.2",
        "libutil.so.1",
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
        "libGL.so.1",
        "libEGL.so.1",
        "libGLX.so.0",
        "libOpenGL.so.0",
        "libGLdispatch.so.0",
    }
    pending = []
    for path in directory.rglob("*"):
        if path.is_file():
            with path.open("rb") as handle:
                if handle.read(4) == b"\x7fELF":
                    pending.append(path)
    needed = {"libsecret-1.so.0"}
    visited = set()
    copied = {}
    while pending or needed:
        if pending:
            needed.update(inspect_elf(pending.pop(), architecture)["needed"])
        for name in sorted(needed - visited):
            visited.add(name)
            if name in host:
                continue
            destination = directory / "lib" / name
            if destination.exists():
                continue
            source = libraries.get(name)
            if source is None or not source.is_file():
                raise ValueError(f"Missing dynamic dependency: {name}")
            install_file(source, destination, 0o644)
            copied[destination] = source
            pending.append(destination)
        needed.clear()
    return copied


def bundle_gtk_resources(appdir: Path, share: Path = Path("/usr/share")) -> dict[Path, Path]:
    """Bundle GTK 3 data only, never the build machine's complete schema database."""
    schemas = appdir / "usr/share/glib-2.0/schemas"
    copied = {}
    sources = [share / "glib-2.0/schemas" / name for name in GTK_SCHEMAS]
    for relative in GTK_RESOURCE_DIRECTORIES:
        directory = share / relative
        if not directory.is_dir() or directory.is_symlink():
            raise ValueError(f"Missing GTK runtime data: {relative}")
        sources.extend(
            path
            for path in sorted(directory.rglob("*"))
            if (path.is_file() or path.is_symlink()) and path.name != "icon-theme.cache"
        )
    if not (share / "icons/Adwaita/index.theme").is_file():
        raise ValueError("Missing GTK fallback icon theme")
    for source in sources:
        if not source.is_file() or not source.resolve().is_relative_to(share.resolve()):
            raise ValueError("GTK runtime data is missing or escapes its data directory")
        destination = appdir / "usr/share" / source.relative_to(share)
        # Dereference only verified in-tree data links. No host absolute links or
        # generated icon cache paths may survive inside the relocatable image.
        install_file(source, destination)
        copied[destination] = source
    run(["glib-compile-schemas", "--strict", schemas])
    compiled = schemas / "gschemas.compiled"
    if not compiled.is_file() or compiled.is_symlink() or not compiled.stat().st_size:
        raise ValueError("GTK runtime schemas were not compiled")
    compiled.chmod(0o644)
    return copied


def distribution_owners(sources: set[Path]) -> dict[Path, str]:
    """Resolve dpkg ownership, including /lib versus /usr/lib merged-root aliases."""
    candidates = {}
    for source in sorted(sources):
        names = {source, source.resolve()}
        for path in tuple(names):
            if path.is_relative_to("/usr/lib"):
                names.add(Path("/lib") / path.relative_to("/usr/lib"))
            elif path.is_relative_to("/lib"):
                names.add(Path("/usr/lib") / path.relative_to("/lib"))
        for path in names:
            candidates.setdefault(str(path), set()).add(source)
    found: dict[Path, set[str]] = {source: set() for source in sources}
    paths = sorted(candidates)
    for offset in range(0, len(paths), 128):
        try:
            output = run(["dpkg-query", "--search", *paths[offset : offset + 128]])
        except subprocess.CalledProcessError as error:
            # One absent usr-merge alias makes dpkg-query exit 1 even when other
            # paths matched. Accept only exact matching records from its output.
            if error.returncode != 1:
                raise
            output = error.output
        for line in output.splitlines():
            package, separator, name = line.partition(": ")
            if separator and name in candidates and DEBIAN_PACKAGE.fullmatch(package):
                for source in candidates[name]:
                    found[source].add(package)
    if any(len(packages) != 1 for packages in found.values()):
        raise ValueError("Bundled runtime file has missing or ambiguous distribution ownership")
    return {source: next(iter(packages)) for source, packages in found.items()}


def bundle_distribution_licenses(
    appdir: Path,
    copied: dict[Path, Path],
    documentation: Path = Path("/usr/share/doc"),
    common_licenses: Path = Path("/usr/share/common-licenses"),
) -> None:
    """Retain the distribution's notices and referenced license texts for every copied file."""
    owners = distribution_owners(set(copied.values()))
    packages = sorted(set(owners.values()))
    versions = {}
    for line in run(
        ["dpkg-query", "--show", "--showformat=${binary:Package}\t${Version}\n", *packages]
    ).splitlines():
        package, separator, version = line.partition("\t")
        if separator and package in packages and re.fullmatch(r"[0-9][A-Za-z0-9.+:~\-]*", version):
            versions[package] = version
    if set(versions) != set(packages):
        raise ValueError("Missing bundled distribution package version")
    notices = []
    for package in packages:
        relative = Path("usr/share/doc") / package.split(":", 1)[0] / "copyright"
        source = documentation / package.split(":", 1)[0] / "copyright"
        if not source.is_file() or not source.stat().st_size:
            raise ValueError(f"Missing distribution copyright: {package}")
        install_file(source, appdir / relative)
        references = re.findall(
            r"/usr/share/common-licenses/([A-Za-z0-9][A-Za-z0-9.+-]*)",
            source.read_text(encoding="utf-8"),
        )
        # Debian notices sometimes end a sentence immediately after the path.
        for name in sorted({reference.rstrip(".") for reference in references}):
            license_file = common_licenses / name
            if not license_file.exists() and name in COMMON_LICENSE_ALIASES:
                license_file = common_licenses / COMMON_LICENSE_ALIASES[name]
            if not license_file.is_file() or not license_file.stat().st_size:
                raise ValueError(f"Missing referenced distribution license: {name} ({package})")
            install_file(license_file, appdir / "usr/share/common-licenses" / name)
        notices.append(
            {
                "package": package,
                "version": versions[package],
                "copyright": relative.as_posix(),
                "files": sorted(
                    destination.relative_to(appdir).as_posix()
                    for destination, original in copied.items()
                    if owners[original] == package
                ),
            }
        )
    write(
        appdir / "usr/share/doc/usque/bundled-distribution-files.json",
        json.dumps(notices, sort_keys=True, indent=2) + "\n",
    )


def entry(path: Path, name: str) -> dict[str, object]:
    if not path.is_file() or path.is_symlink() or not 0 < path.stat().st_size <= 512 * 1024 * 1024:
        raise ValueError(f"Invalid manifest input: {name}")
    return {"name": name, "size": path.stat().st_size, "sha256": digest(path)}


def sign_manifest(value: dict[str, object], output: Path, key: Path, *, test_signing: bool) -> None:
    # OpenSSL handles the private key directly; only its public DER is inspected.
    with tempfile.TemporaryDirectory(prefix="usque-sign-public-") as temporary:
        public = Path(temporary) / "public.der"
        run(["openssl", "pkey", "-in", key, "-pubout", "-outform", "DER", "-out", public])
        public_der = public.read_bytes()
    if len(public_der) != 44 or public_der[:12].hex() != "302a300506032b6570032100":
        raise ValueError("Linux signing identity must be Ed25519")
    expected = (ROOT / "packaging/linux/release-public-key.hex").read_text().strip()
    if not test_signing and public_der[-32:].hex() != expected:
        raise ValueError("Linux signing key does not match the checked-in release public key")
    output.write_text(
        json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8"
    )
    run(
        [
            "openssl",
            "pkeyutl",
            "-sign",
            "-rawin",
            "-inkey",
            key,
            "-in",
            output,
            "-out",
            str(output) + ".sig",
        ]
    )
    if Path(str(output) + ".sig").stat().st_size != 64:
        raise ValueError("Invalid Ed25519 signature length")


def manifest(
    kind: str, version: str, architecture: str, entries: list[dict[str, object]]
) -> dict[str, object]:
    return {
        "schema_version": 1,
        "kind": kind,
        "version": version,
        "architecture": architecture,
        "protocol_version": 3,
        "entries": entries,
    }


def download_dependency(name: str, architecture: str, directory: Path) -> Path:
    lock = json.loads((ROOT / "tool/linux_build_dependencies.json").read_text())[name][architecture]
    destination = directory / f"{name}-{architecture}"
    if not destination.exists():
        # URLs are fixed, HTTPS, and independently hash-locked in the repository.
        with urllib.request.urlopen(lock["url"], timeout=120) as response:  # noqa: S310
            with destination.open("xb") as output:
                shutil.copyfileobj(response, output)
    if digest(destination) != lock["sha256"]:
        raise ValueError(f"Pinned {name} hash mismatch")
    destination.chmod(0o755)
    return destination


def install_file(source: Path, target: Path, mode: int = 0o644) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    target.chmod(mode)


def payload_inventory(directory: Path, *, skip_control: bool = False) -> dict[str, dict]:
    """Hash package contents without running any payload or following symlinks."""
    inventory = {}
    for path in sorted(directory.rglob("*")):
        relative = path.relative_to(directory).as_posix()
        if skip_control and relative.split("/", 1)[0] == "DEBIAN":
            continue
        metadata = path.lstat()
        if not stat.S_ISLNK(metadata.st_mode) and metadata.st_mode & (
            stat.S_ISUID | stat.S_ISGID | stat.S_IWGRP | stat.S_IWOTH
        ):
            raise ValueError(f"Unsafe package mode: {relative}")
        if path.is_symlink():
            if not path.resolve().is_relative_to(directory.resolve()):
                raise ValueError(f"Escaping package symlink: {relative}")
            inventory[relative] = {"link": os.readlink(path)}
        elif path.is_file():
            inventory[relative] = {
                "sha256": digest(path),
                "size": metadata.st_size,
                "mode": stat.S_IMODE(metadata.st_mode),
            }
        elif not path.is_dir():
            raise ValueError(f"Special file in package: {relative}")
    return inventory


def inspect_archive(root: Path, archive: Path, architecture: str, package_format: str) -> None:
    """Round-trip each built archive and compare every byte, mode and link."""
    expected = payload_inventory(root, skip_control=True)
    with tempfile.TemporaryDirectory(prefix="usque-inspect-package-") as directory:
        extracted = Path(directory) / "payload"
        extracted.mkdir()
        if package_format == "deb":
            run(["dpkg-deb", "--extract", archive, extracted])
            control = Path(directory) / "control"
            run(["dpkg-deb", "--control", archive, control])
            if payload_inventory(control) != payload_inventory(root / "DEBIAN"):
                raise ValueError("Debian maintainer scripts differ from staged input")
        elif package_format == "rpm":
            # libarchive reads RPM/cpio; no scriptlet or packaged binary runs.
            run(["bsdtar", "--extract", "--no-same-owner", "--file", archive, "-C", extracted])
        else:
            raise ValueError("Unsupported package archive")
        if payload_inventory(extracted) != expected:
            raise ValueError(f"{package_format} content differs from staged input")
        inspect_bundle(extracted / "usr/lib/usque", architecture)


def write(path: Path, text: str, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    path.chmod(mode)


def record(root: Path, version: str, architecture: str, package_format: str) -> None:
    value = {
        "schema_version": 1,
        "version": version,
        "architecture": architecture,
        "protocol_version": 3,
        "format": package_format,
        "engine_sha256": digest(root / "usr/lib/usque/usque-engine"),
    }
    write(root / "etc/usque/install.json", json.dumps(value, sort_keys=True) + "\n")


def preinstall(package_format: str, version: str = "0.0.0", architecture: str = "amd64") -> str:
    # Exact fixed-format record check before dpkg/rpm may overwrite shared paths.
    return f"""#!/bin/sh
set -eu
if [ -e /etc/usque/install.json ] || [ -L /etc/usque/install.json ]; then
  /usr/bin/python3 -I - <<'PY'
import json, os, re, stat
for path in ("/etc/usque", "/etc/usque/install.json"):
    info = os.lstat(path)
    if info.st_uid != 0 or info.st_mode & 0o022 or stat.S_ISLNK(info.st_mode):
        raise SystemExit("Untrusted Usque installation record")
fd = os.open("/etc/usque/install.json", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
with os.fdopen(fd, "rb") as stream:
    if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
        raise SystemExit("Invalid Usque installation record")
    data = stream.read(8193)
if len(data) > 8192:
    raise SystemExit("Oversized Usque installation record")
d = json.loads(data)
if not (d["schema_version"] == 1 and d["format"] == "{package_format}" and d["architecture"] == "{architecture}"):
    raise SystemExit("Mixed Usque installations are not supported")
if re.fullmatch(r"(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)", d["version"]) is None:
    raise SystemExit("Invalid installed version")
if "{version}" != "0.0.0" and tuple(map(int, d["version"].split("."))) > tuple(map(int, "{version}".split("."))):
    raise SystemExit("Usque version rollback is not supported")
PY
  /usr/lib/usque/usque-update --prepare-remove
elif [ -e /usr/lib/usque/usque-agent ]; then
  echo 'Unrecognized Usque installation; refusing to overwrite it.' >&2
  exit 1
fi
"""


POSTINSTALL = """#!/bin/sh
set -eu
update-desktop-database /usr/share/applications
systemctl daemon-reload
systemctl enable --now usque-agent.service
"""
POSTREMOVE = """#!/bin/sh
set -eu
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database /usr/share/applications
fi
systemctl daemon-reload
"""
PREREMOVE = """#!/bin/sh
set -eu
if [ -x /usr/lib/usque/usque-update ]; then
  case "${1:-}" in
    remove|0) /usr/lib/usque/usque-update --prepare-remove --disable-service ;;
    *) /usr/lib/usque/usque-update --prepare-remove ;;
  esac
fi
"""


def build_deb(root: Path, output: Path, version: str, architecture: str) -> None:
    record(root, version, architecture, "deb")
    write(
        root / "DEBIAN/control",
        f"""Package: usque
Version: {version}
Architecture: {architecture}
Maintainer: Usque contributors <65408965+GeorgeXie2333@users.noreply.github.com>
Section: net
Priority: optional
Depends: libc6 (>= 2.35), libstdc++6 (>= 12), libgtk-3-0, libsecret-1-0, libgl1, systemd, policykit-1 | polkitd, policykit-1 | pkexec, nftables, iproute2, python3, desktop-file-utils
Recommends: gnome-keyring
Description: Desktop client compatible with Cloudflare WARP
 Privilege-separated desktop client with a system service.
""",
    )
    write(root / "DEBIAN/preinst", preinstall("deb", version, architecture), 0o755)
    write(root / "DEBIAN/postinst", POSTINSTALL, 0o755)
    write(root / "DEBIAN/prerm", PREREMOVE, 0o755)
    write(root / "DEBIAN/postrm", POSTREMOVE, 0o755)
    run(["dpkg-deb", "--root-owner-group", "--build", root, output])
    if run(["dpkg-deb", "--field", output, "Architecture"]).strip() != architecture:
        raise ValueError("Debian package architecture mismatch")
    inspect_archive(root, output, architecture, "deb")


def rpm_scriptlets(version: str, architecture: str) -> dict[str, str]:
    return {
        "PREIN": preinstall("rpm", version, architecture).removeprefix("#!/bin/sh").strip(),
        "POSTIN": POSTINSTALL.removeprefix("#!/bin/sh").strip(),
        "PREUN": 'if [ "$1" -eq 0 ]; then\n' + PREREMOVE.removeprefix("#!/bin/sh").strip() + "\nfi",
        "POSTUN": POSTREMOVE.removeprefix("#!/bin/sh").strip(),
    }


def inspect_rpm_metadata(archive: Path, version: str, architecture: str) -> None:
    """Read RPM headers only; never invoke an installed or packaged scriptlet."""
    expected = {
        "NAME": "usque",
        "VERSION": version,
        "RELEASE": "1",
        "ARCH": ARCHITECTURES[architecture][1],
    }
    for tag, value in expected.items():
        if run(["rpm", "-qp", "--queryformat", f"%{{{tag}}}", archive]) != value:
            raise ValueError(f"RPM {tag} differs from staged package identity")
    for tag, script in rpm_scriptlets(version, architecture).items():
        actual = run(["rpm", "-qp", "--queryformat", f"%{{{tag}}}", archive])
        if actual.strip() != script:
            raise ValueError(f"RPM {tag} scriptlet differs from staged input")
        # Program tags are arrays: compare every element, including arguments,
        # so an extra shell flag or another interpreter cannot hide after sh.
        program = run(["rpm", "-qp", "--queryformat", f"[%{{{tag}PROG}}\\n]", archive])
        if program != "/bin/sh\n":
            raise ValueError(f"RPM {tag} interpreter differs from staged input")
    for tag in (
        "EPOCH",
        "PRETRANS",
        "POSTTRANS",
        "VERIFYSCRIPT",
        "TRIGGERSCRIPTS",
        "FILETRIGGERSCRIPTS",
        "TRANSFILETRIGGERSCRIPTS",
    ):
        query = f"%|{tag}?{{present}}:{{absent}}|"
        if run(["rpm", "-qp", "--queryformat", query, archive]) != "absent":
            raise ValueError(f"Unexpected RPM {tag} metadata")


def build_rpm(root: Path, output: Path, version: str, architecture: str, temporary: Path) -> None:
    record(root, version, architecture, "rpm")
    rpm_arch = ARCHITECTURES[architecture][1]
    spec = temporary / "usque.spec"
    scripts = rpm_scriptlets(version, architecture)
    files = [
        "/" + str(path.relative_to(root))
        for path in sorted(root.rglob("*"))
        if path.is_file() or path.is_symlink()
    ]
    write(
        spec,
        f"""Name: usque
Version: {version}
Release: 1
Summary: Desktop client compatible with Cloudflare WARP
License: MIT
URL: https://github.com/GeorgeXie2333/usque-app
BuildArch: {rpm_arch}
Requires: gtk3, libsecret, mesa-libGL, systemd, polkit, nftables, iproute, python3, desktop-file-utils
%description
Privilege-separated desktop client with a system service.
%prep
%build
%install
mkdir -p %{{buildroot}}
cp -a {shlex.quote(str(root))}/. %{{buildroot}}/
%pre -p /bin/sh
{scripts["PREIN"]}
%post -p /bin/sh
{scripts["POSTIN"]}
%preun -p /bin/sh
{scripts["PREUN"]}
%postun -p /bin/sh
{scripts["POSTUN"]}
%files
%defattr(-,root,root,-)
{os.linesep.join(files)}
""",
    )
    run(
        [
            "rpmbuild",
            "-bb",
            "--target",
            rpm_arch,
            "--define",
            f"_topdir {temporary / 'rpm'}",
            "--define",
            "_build_id_links none",
            "--define",
            "debug_package %{nil}",
            "--define",
            "__os_install_post %{nil}",
            spec,
        ]
    )
    packages = list((temporary / "rpm/RPMS").rglob("*.rpm"))
    if len(packages) != 1:
        raise ValueError("Expected one RPM")
    shutil.copyfile(packages[0], output)
    inspect_rpm_metadata(output, version, architecture)
    inspect_archive(root, output, architecture, "rpm")


def build(args: argparse.Namespace) -> None:
    version = args.version
    artifact_name(version, args.architecture, "deb")
    if platform.machine() != ARCHITECTURES[args.architecture][1]:
        raise ValueError("Linux packages require a matching native build host")
    args.output.mkdir(parents=True, exist_ok=True)
    args.output = args.output.resolve()
    dependencies = args.output / "build-tools"
    dependencies.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="usque-linux-package-", dir=args.output) as directory:
        temporary = Path(directory)
        root = temporary / "root"
        payload = root / "usr/lib/usque"
        shutil.copytree(args.bundle, payload)
        for name in ("usque-engine", "usque-agent", "usque-update"):
            install_file(args.rust / name, payload / name, 0o755)
        report = inspect_bundle(payload, args.architecture)
        write(
            args.output / f"linux-{args.architecture}-elf.json", json.dumps(report, indent=2) + "\n"
        )
        for source, target in (
            ("usque-agent.service", SERVICE_FILES[3]),
            ("io.github.georgexie2333.usque.policy", SERVICE_FILES[4]),
            (
                "io.github.georgexie2333.usque.desktop",
                "usr/share/applications/io.github.georgexie2333.usque.desktop",
            ),
        ):
            install_file(ROOT / "packaging/linux" / source, root / target)
        install_file(
            ROOT / "apps/usque_gui/assets/branding/usque-ui-icon.png",
            root / "usr/share/icons/hicolor/256x256/apps/io.github.georgexie2333.usque.png",
        )
        install_file(ROOT / "LICENSE.md", root / "usr/share/doc/usque/copyright")
        write(root / "usr/bin/usque", '#!/bin/sh\nexec /usr/lib/usque/usque "$@"\n', 0o755)
        service = manifest(
            "service",
            version,
            args.architecture,
            [entry(root / name, name) for name in SERVICE_FILES],
        )
        sign_manifest(
            service,
            root / "linux-service-manifest.json",
            args.signing_key,
            test_signing=args.test_signing,
        )
        appdir = temporary / "Usque.AppDir"
        shutil.copytree(root, appdir)
        copied = bundle_libraries(appdir / "usr/lib/usque", args.architecture)
        copied.update(bundle_gtk_resources(appdir))
        bundle_distribution_licenses(appdir, copied)
        install_file(ROOT / "packaging/linux/AppRun", appdir / "AppRun", 0o755)
        install_file(
            ROOT / "packaging/linux/io.github.georgexie2333.usque.desktop",
            appdir / "io.github.georgexie2333.usque.desktop",
        )
        install_file(
            ROOT / "apps/usque_gui/assets/branding/usque-ui-icon.png",
            appdir / "io.github.georgexie2333.usque.png",
        )
        appimagetool = download_dependency("appimagetool", args.architecture, dependencies)
        runtime = download_dependency("runtime", args.architecture, dependencies)
        run([appimagetool, "--appimage-extract"], cwd=temporary)
        run(
            [
                temporary / "squashfs-root/AppRun",
                "--runtime-file",
                runtime,
                "--no-appstream",
                appdir,
                args.output / artifact_name(version, args.architecture, "AppImage"),
            ]
        )
        # The pinned runtime is prepended verbatim in size. Read the SquashFS
        # directly; this check never executes the newly built AppImage.
        extracted_appdir = temporary / "appimage-check"
        run(
            [
                "unsquashfs",
                "-no-progress",
                "-o",
                str(runtime.stat().st_size),
                "-d",
                extracted_appdir,
                args.output / artifact_name(version, args.architecture, "AppImage"),
            ]
        )
        if payload_inventory(extracted_appdir) != payload_inventory(appdir):
            raise ValueError("AppImage content differs from staged input")
        inspect_bundle(extracted_appdir / "usr/lib/usque", args.architecture)
        # Only AppImage needs the service bootstrap manifests at its root.
        (root / "linux-service-manifest.json").unlink()
        (root / "linux-service-manifest.json.sig").unlink()
        debroot = temporary / "deb"
        shutil.copytree(root, debroot)
        build_deb(
            debroot,
            args.output / artifact_name(version, args.architecture, "deb"),
            version,
            args.architecture,
        )
        build_rpm(
            root,
            args.output / artifact_name(version, args.architecture, "rpm"),
            version,
            args.architecture,
            temporary,
        )
    names = [
        artifact_name(version, args.architecture, package_format) for package_format in FORMATS
    ]
    signed = manifest(
        "packages", version, args.architecture, [entry(args.output / name, name) for name in names]
    )
    sign_manifest(
        signed,
        args.output / f"usque-v{version}-linux-{args.architecture}.manifest.json",
        args.signing_key,
        test_signing=args.test_signing,
    )
    if args.test_signing:
        write(
            args.output / "TEST-IDENTITY-DO-NOT-PUBLISH",
            "Compile-only packages signed with an ephemeral test identity.\n",
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--architecture", choices=ARCHITECTURES, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--signing-key", type=Path, required=True)
    parser.add_argument("--test-signing", action="store_true")
    build(parser.parse_args())


if __name__ == "__main__":
    main()
