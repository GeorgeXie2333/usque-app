from __future__ import annotations

import json
import os
import shutil
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import linux_package
import linux_release_contract
import release_contract


class LinuxPackageTests(unittest.TestCase):
    @unittest.skipUnless(
        shutil.which("update-desktop-database"), "desktop-file-utils required for MIME fixture"
    )
    def test_desktop_database_routes_the_zero_trust_callback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            applications = Path(directory) / "applications"
            applications.mkdir()
            name = "io.github.georgexie2333.usque.desktop"
            shutil.copyfile(linux_package.ROOT / "packaging/linux" / name, applications / name)
            linux_package.run(["update-desktop-database", applications])
            cache = (applications / "mimeinfo.cache").read_text()
            self.assertIn(f"x-scheme-handler/com.cloudflare.warp={name};", cache)

    def test_six_distinct_names_and_native_architecture_mapping(self) -> None:
        names = {
            linux_package.artifact_name("1.2.3", arch, kind)
            for arch in linux_package.ARCHITECTURES
            for kind in linux_package.FORMATS
        }
        self.assertEqual(len(names), 6)
        self.assertEqual(linux_package.ARCHITECTURES["amd64"][1], "x86_64")
        self.assertEqual(linux_package.ARCHITECTURES["arm64"][1], "aarch64")
        self.assertIn("usque-v1.2.3-linux-amd64.AppImage", names)
        for version in ("../1.2.3", "01.2.3", "1.2.3\n", "1.2.3-beta.1"):
            with self.assertRaises(ValueError):
                linux_package.artifact_name(version, "amd64", "deb")

    def test_elf_rejects_architecture_new_libc_and_new_x86_baselines(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "binary"
            header = bytearray(64)
            header[:6] = b"\x7fELF\x02\x01"
            struct.pack_into("<H", header, 18, 62)
            path.write_bytes(header)
            with patch.object(
                linux_package,
                "run",
                side_effect=[
                    "(NEEDED) [libc.so.6]",
                    "GLIBC_2.35 GLIBCXX_3.4.30",
                    "x86 ISA needed: x86-64-baseline",
                ],
            ):
                self.assertEqual(linux_package.inspect_elf(path, "amd64")["needed"], ["libc.so.6"])
            with self.assertRaises(ValueError):
                linux_package.inspect_elf(path, "arm64")
            for versions, notes in (
                ("GLIBC_2.36", ""),
                ("GLIBCXX_3.4.31", ""),
                ("GLIBC_PRIVATE", ""),
                ("GLIBC_2.35", "x86-64-v2"),
            ):
                with (
                    patch.object(linux_package, "run", side_effect=["", versions, notes]),
                    self.assertRaises(ValueError),
                ):
                    linux_package.inspect_elf(path, "amd64")

    def test_bundle_rejects_preview_and_missing_production_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                linux_package.inspect_bundle(Path(directory), "amd64")

    def test_library_closure_records_every_distribution_source_and_excludes_host_glibc(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = root / "bundle"
            linux_package.write(payload / "usque", "\x7fELFinert")
            secret = root / "libsecret-1.so.0"
            secret.write_bytes(b"\x7fELFsecret")
            gtk = root / "libgtk-3.so.0"
            gtk.write_bytes(b"\x7fELFgtk")
            cache = f"\tlibsecret-1.so.0 (libc6) => {secret}\n\tlibgtk-3.so.0 (libc6) => {gtk}\n"
            with (
                patch.object(linux_package, "run", return_value=cache),
                patch.object(
                    linux_package,
                    "inspect_elf",
                    return_value={"needed": ["libgtk-3.so.0", "libc.so.6"]},
                ),
            ):
                copied = linux_package.bundle_libraries(payload, "amd64")
            self.assertEqual(
                copied,
                {payload / "lib/libsecret-1.so.0": secret, payload / "lib/libgtk-3.so.0": gtk},
            )
            self.assertFalse((payload / "lib/libc.so.6").exists())

    @unittest.skipUnless(shutil.which("glib-compile-schemas"), "GLib schema compiler required")
    def test_gtk_schemas_compile_in_isolation_and_only_selected_static_resources_are_copied(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            share = root / "host-share"
            appdir = root / "AppDir"
            for name in linux_package.GTK_SCHEMAS:
                schema_id = name.removesuffix(".gschema.xml")
                linux_package.write(
                    share / "glib-2.0/schemas" / name,
                    f'<schemalist><schema id="{schema_id}" path="/fixture/">'
                    '<key name="enabled" type="b"><default>true</default></key>'
                    "</schema></schemalist>",
                )
            linux_package.write(share / "glib-2.0/schemas/gschemas.compiled", "host database")
            linux_package.write(share / "glib-2.0/schemas/unrelated.gschema.xml", "invalid XML")
            for name in linux_package.GTK_RESOURCE_DIRECTORIES:
                linux_package.write(share / name / "fixture", "static data")
            linux_package.write(share / "icons/Adwaita/index.theme", "[Icon Theme]\nName=Adwaita\n")
            linux_package.write(share / "icons/Adwaita/icon-theme.cache", "host cache")
            link = share / "icons/Adwaita/linked-icon"
            link.symlink_to("fixture")
            copied = linux_package.bundle_gtk_resources(appdir, share)
            schemas = appdir / "usr/share/glib-2.0/schemas"
            self.assertEqual(
                {path.name for path in schemas.iterdir()},
                {*linux_package.GTK_SCHEMAS, "gschemas.compiled"},
            )
            self.assertNotEqual((schemas / "gschemas.compiled").read_bytes(), b"host database")
            self.assertEqual(len(copied), len(linux_package.GTK_SCHEMAS) + 6)
            self.assertFalse((appdir / "usr/share/icons/Adwaita/icon-theme.cache").exists())
            self.assertFalse((appdir / "usr/share/icons/Adwaita/linked-icon").is_symlink())
            link.unlink()
            link.symlink_to(root / "outside")
            (root / "outside").write_text("external data")
            with self.assertRaises(ValueError):
                linux_package.bundle_gtk_resources(root / "escaped", share)
            link.unlink()
            (share / "glib-2.0/schemas" / linux_package.GTK_SCHEMAS[0]).unlink()
            with self.assertRaises(ValueError):
                linux_package.bundle_gtk_resources(root / "missing", share)

    @unittest.skipUnless(shutil.which("sh"), "POSIX shell required for inert launcher fixture")
    def test_apprun_limits_schema_path_and_keeps_host_desktop_data_search(self) -> None:
        with tempfile.TemporaryDirectory(prefix="usque appdir ") as directory:
            appdir = Path(directory)
            linux_package.install_file(
                linux_package.ROOT / "packaging/linux/AppRun", appdir / "AppRun", 0o755
            )
            linux_package.write(
                appdir / "usr/lib/usque/usque",
                '#!/bin/sh\nprintf "%s\\n" "$GSETTINGS_SCHEMA_DIR" "$GTK_DATA_PREFIX" '
                '"$XDG_DATA_DIRS" "$@"\n',
                0o755,
            )
            for data_dirs in ("", "/fixture/desktop/share"):
                with patch.dict(
                    os.environ,
                    {"XDG_DATA_DIRS": data_dirs, "GSETTINGS_SCHEMA_DIR": "/unrelated/schemas"},
                ):
                    result = linux_package.run([appdir / "AppRun", "inert argument"]).splitlines()
                self.assertEqual(result[0], str(appdir / "usr/share/glib-2.0/schemas"))
                self.assertEqual(result[1], str(appdir / "usr"))
                self.assertEqual(
                    result[2],
                    str(appdir / "usr/share") + ":" + (data_dirs or "/usr/local/share:/usr/share"),
                )
                self.assertEqual(result[3], "inert argument")

    def test_distribution_ownership_accepts_usr_merge_and_rejects_missing_or_ambiguous_files(
        self,
    ) -> None:
        source = Path("/usr/lib/usque-inert-fixture.so.1")
        output = "libfixture1:amd64: /lib/usque-inert-fixture.so.1\n"
        with patch.object(
            linux_package,
            "run",
            side_effect=subprocess.CalledProcessError(1, "dpkg-query", output=output),
        ) as query:
            self.assertEqual(
                linux_package.distribution_owners({source}), {source: "libfixture1:amd64"}
            )
            self.assertEqual(query.call_args.args[0][:2], ["dpkg-query", "--search"])
        for response in ("", output + "libother1:amd64: /usr/lib/usque-inert-fixture.so.1\n"):
            with (
                patch.object(linux_package, "run", return_value=response),
                self.assertRaises(ValueError),
            ):
                linux_package.distribution_owners({source})

    def test_distribution_copyright_and_referenced_licenses_are_retained_with_provenance(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            appdir = root / "AppDir"
            doc = root / "doc"
            common = root / "common-licenses"
            original = Path("/usr/lib/libfixture.so.1")
            copied = {appdir / "usr/lib/usque/lib/libfixture.so.1": original}
            license_text = "Fixture copyright. See /usr/share/common-licenses/LGPL-2.1.\n"
            linux_package.write(doc / "libfixture1/copyright", license_text)
            linux_package.write(common / "LGPL-2.1", "Fixture full license\n")
            with (
                patch.object(
                    linux_package,
                    "distribution_owners",
                    return_value={original: "libfixture1:amd64"},
                ),
                patch.object(
                    linux_package, "run", return_value="libfixture1:amd64\t1.2.3-1ubuntu1\n"
                ),
            ):
                linux_package.bundle_distribution_licenses(appdir, copied, doc, common)
                self.assertEqual(
                    (appdir / "usr/share/doc/libfixture1/copyright").read_text(), license_text
                )
                self.assertEqual(
                    (appdir / "usr/share/common-licenses/LGPL-2.1").read_text(),
                    "Fixture full license\n",
                )
                notices = json.loads(
                    (appdir / "usr/share/doc/usque/bundled-distribution-files.json").read_text()
                )
                self.assertEqual(notices[0]["package"], "libfixture1:amd64")
                self.assertEqual(notices[0]["version"], "1.2.3-1ubuntu1")
                self.assertEqual(notices[0]["files"], ["usr/lib/usque/lib/libfixture.so.1"])
                (common / "LGPL-2.1").unlink()
                with self.assertRaisesRegex(ValueError, "referenced distribution license"):
                    linux_package.bundle_distribution_licenses(appdir, copied, doc, common)
                (doc / "libfixture1/copyright").unlink()
                with self.assertRaisesRegex(ValueError, "distribution copyright"):
                    linux_package.bundle_distribution_licenses(appdir, copied, doc, common)

    def test_package_hooks_restore_before_removal_and_reject_mixed_sources(self) -> None:
        self.assertIn("--prepare-remove", linux_package.PREREMOVE)
        for package_format in ("deb", "rpm"):
            script = linux_package.preinstall(package_format)
            self.assertIn(f'd["format"] == "{package_format}"', script)
            self.assertIn("refusing to overwrite", script)
            self.assertIn("set -eu", script)
        self.assertNotIn("purge-user-data", linux_package.PREREMOVE)

    @unittest.skipUnless(shutil.which("sh"), "POSIX shell required for inert hook fixtures")
    def test_final_remove_disables_service_but_upgrade_and_deconfigure_do_not(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            helper = root / "fake-helper"
            linux_package.write(helper, '#!/bin/sh\nprintf "%s\\n" "$@"\n', 0o755)
            script = root / "hook"
            linux_package.write(
                script,
                linux_package.PREREMOVE.replace("/usr/lib/usque/usque-update", str(helper)),
                0o755,
            )
            for action in ("remove", "0", "upgrade", "1", "deconfigure"):
                arguments = linux_package.run(["sh", script, action]).splitlines()
                self.assertEqual(arguments[0], "--prepare-remove")
                self.assertEqual("--disable-service" in arguments, action in ("remove", "0"))

    def test_payload_inventory_rejects_escaping_links_and_privileged_modes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            payload = root / "binary"
            payload.write_bytes(b"inert fixture")
            payload.chmod(0o755)
            self.assertEqual(linux_package.payload_inventory(root)["binary"]["mode"], 0o755)
            for mode in (0o4755, 0o2775, 0o775, 0o777):
                payload.chmod(mode)
                with self.assertRaises(ValueError):
                    linux_package.payload_inventory(root)
            payload.chmod(0o755)
            link = root / "internal-link"
            link.symlink_to("binary")
            self.assertEqual(
                linux_package.payload_inventory(root)["internal-link"], {"link": "binary"}
            )
            link.unlink()
            payload.unlink()
            payload.symlink_to("../outside")
            with self.assertRaises(ValueError):
                linux_package.payload_inventory(root)

    def test_rpm_headers_and_all_install_remove_scriptlets_are_verified_without_execution(
        self,
    ) -> None:
        for architecture in linux_package.ARCHITECTURES:
            fields = {
                "%{NAME}": "usque",
                "%{VERSION}": "1.2.3",
                "%{RELEASE}": "1",
                "%{ARCH}": linux_package.ARCHITECTURES[architecture][1],
            }
            for tag, script in linux_package.rpm_scriptlets("1.2.3", architecture).items():
                fields[f"%{{{tag}}}"] = script + "\n"
                fields[f"[%{{{tag}PROG}}\\n]"] = "/bin/sh\n"
            for tag in (
                "EPOCH",
                "PRETRANS",
                "POSTTRANS",
                "VERIFYSCRIPT",
                "TRIGGERSCRIPTS",
                "FILETRIGGERSCRIPTS",
                "TRANSFILETRIGGERSCRIPTS",
            ):
                fields[f"%|{tag}?{{present}}:{{absent}}|"] = "absent"

            def query(arguments: list, fields: dict[str, str] = fields) -> str:
                self.assertEqual(arguments[:3], ["rpm", "-qp", "--queryformat"])
                self.assertEqual(arguments[-1], Path("inert.rpm"))
                return fields[arguments[3]]

            with patch.object(linux_package, "run", side_effect=query):
                linux_package.inspect_rpm_metadata(Path("inert.rpm"), "1.2.3", architecture)
                for tag in fields:
                    with self.subTest(architecture=architecture, tag=tag):
                        original = fields[tag]
                        fields[tag] = original + "tampered"
                        with self.assertRaises(ValueError):
                            linux_package.inspect_rpm_metadata(
                                Path("inert.rpm"), "1.2.3", architecture
                            )
                        fields[tag] = original

    @unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb required for inert archive fixture")
    def test_debian_round_trip_verifies_payload_and_maintainer_scripts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "root"
            linux_package.write(root / "usr/lib/usque/usque-engine", "inert engine\n", 0o755)
            archive = Path(directory) / "fixture.deb"
            with patch.object(linux_package, "inspect_bundle"):
                linux_package.build_deb(root, archive, "1.2.3", "amd64")
                (root / "usr/lib/usque/usque-engine").write_text("changed engine\n")
                with self.assertRaisesRegex(ValueError, "content differs"):
                    linux_package.inspect_archive(root, archive, "amd64", "deb")
                (root / "DEBIAN/prerm").write_text("changed hook\n")
                with self.assertRaisesRegex(ValueError, "maintainer scripts"):
                    linux_package.inspect_archive(root, archive, "amd64", "deb")


@unittest.skipUnless(shutil.which("openssl"), "OpenSSL is required for Ed25519 fixture tests")
class LinuxSignatureTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.key = self.root / "test-key.pem"
        linux_package.run(["openssl", "genpkey", "-algorithm", "ED25519", "-out", self.key])
        public = self.root / "public.der"
        linux_package.run(
            ["openssl", "pkey", "-in", self.key, "-pubout", "-outform", "DER", "-out", public]
        )
        self.public_key = public.read_bytes()[-32:].hex()
        self.path = self.root / "manifest.json"
        entries = []
        for package_format in linux_package.FORMATS:
            name = linux_package.artifact_name("1.2.3", "amd64", package_format)
            (self.root / name).write_bytes(b"test package")
            entries.append(linux_package.entry(self.root / name, name))
        self.value = linux_package.manifest("packages", "1.2.3", "amd64", entries)
        self.sign()

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def sign(self) -> None:
        linux_package.sign_manifest(self.value, self.path, self.key, test_signing=True)

    def verify(self, **kwargs) -> dict:
        return linux_release_contract.verify_signed_manifest(
            self.path, architecture="amd64", version="1.2.3", public_key=self.public_key, **kwargs
        )

    def test_valid_signature_binds_exact_bytes_and_kind(self) -> None:
        self.assertEqual(self.verify(), self.value)
        with self.assertRaises(ValueError):
            self.verify(kind="service")
        self.path.write_bytes(self.path.read_bytes() + b" ")
        with self.assertRaises(ValueError):
            self.verify()

    def test_test_identity_cannot_be_used_for_a_production_signature(self) -> None:
        with self.assertRaises(ValueError):
            linux_package.sign_manifest(self.value, self.path, self.key, test_signing=False)
        with self.assertRaises(ValueError):
            linux_release_contract.verify_signed_manifest(
                self.path, architecture="amd64", version="1.2.3"
            )

    def test_signed_duplicate_path_architecture_and_version_mismatch_rejected(self) -> None:
        for field, value in (
            ("architecture", "arm64"),
            ("version", "1.2.2"),
            ("protocol_version", 2),
        ):
            original = self.value[field]
            self.value[field] = value
            self.sign()
            with self.assertRaises(ValueError):
                self.verify()
            self.value[field] = original
        self.value["entries"][0]["name"] = "../escape"
        self.sign()
        with self.assertRaises(ValueError):
            self.verify()

    def test_six_package_release_contract_requires_signed_metadata(self) -> None:
        for name in release_contract.expected_artifact_names("v1.2.3", include_linux=True):
            (self.root / name).write_bytes(name.encode())
        with self.assertRaises(release_contract.ContractError):
            release_contract.create_manifest(
                self.root, "v1.2.3", "a" * 40, "b" * 64, "c" * 64, include_linux=True
            )
        with patch.object(release_contract, "verify_linux_release", return_value=self.public_key):
            manifest = release_contract.create_manifest(
                self.root, "v1.2.3", "a" * 40, "b" * 64, "c" * 64, include_linux=True
            )
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(len(release_contract.artifact_index(manifest)), 14)
            manifest["artifacts"].pop()
            with self.assertRaises(release_contract.ContractError):
                release_contract.artifact_index(manifest)

    def test_build_dependencies_are_versioned_https_and_hash_pinned(self) -> None:
        values = json.loads((linux_package.ROOT / "tool/linux_build_dependencies.json").read_text())
        for tool in values.values():
            for architecture in linux_package.ARCHITECTURES:
                self.assertTrue(
                    tool[architecture]["url"].startswith("https://github.com/AppImage/")
                )
                self.assertNotIn("/continuous/", tool[architecture]["url"])
                self.assertRegex(tool[architecture]["sha256"], r"^[0-9a-f]{64}$")


if __name__ == "__main__":
    unittest.main()
