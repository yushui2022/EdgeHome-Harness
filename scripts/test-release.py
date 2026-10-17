"""Regression tests for distribution failures; no compiler or network needed."""
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="edgehome-release-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.info = release.version_info()
        self.commit = "a" * 40

    def fixtures(self):
        for spec in release.TARGETS:
            files = {"edgehome-dev": (b"binary fixture", 0o755), "LICENSE": (b"license fixture", 0o644)}
            meta = {**self.info, "source_commit": self.commit, "source_dirty": False,
                    "release_eligible": True, "target": spec["target"],
                    "files": {name: release.digest(data) for name, (data, _) in files.items()}}
            files["release.json"] = (json.dumps(meta).encode(), 0o644)
            name = f"edgehome-dev-{self.info['tag']}-{spec['target']}.{spec['archive']}"
            data = release.archive_bytes(files, spec["archive"])
            (self.root / name).write_bytes(data)
            (self.root / (name + ".sha256")).write_text(f"{release.digest(data)}  {name}\n", encoding="utf-8")

    def assemble(self):
        with patch.object(release, "run", return_value=self.commit):
            release.assemble(self.root, self.info["tag"])

    def mutate_manifest(self, **changes):
        archive = next(self.root.glob("*.zip"))
        unpacked = release.unpack(archive, self.root / "staging")
        meta = json.loads((unpacked / "release.json").read_text())
        meta.update(changes)
        (unpacked / "release.json").write_text(json.dumps(meta))
        files = {p.name: (p.read_bytes(), 0o644) for p in unpacked.iterdir()}
        # Keep the assembly folder strictly limited to its input artifacts.
        for path in unpacked.iterdir():
            path.unlink()
        unpacked.rmdir()
        data = release.archive_bytes(files, "zip")
        archive.write_bytes(data)
        (self.root / (archive.name + ".sha256")).write_text(f"{release.digest(data)}  {archive.name}\n", encoding="utf-8")

    def test_tag_must_equal_cli_version(self):
        for value in ["main", "v999.0.0", self.info["version"], "../../bad"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                release.version_info(value)

    def test_dirty_release_source_rejected(self):
        with patch.object(release, "run", return_value=" M README.md"):
            with self.assertRaises(ValueError):
                release.source_info()
            self.assertTrue(release.source_info(allow_dirty=True)["source_dirty"])

    def test_normalized_archives_and_executable_permissions(self):
        files = {"edgehome-dev": (b"fixture", 0o755), "docs/readme.txt": (b"readme", 0o644)}
        for extension in ["zip", "tar.gz"]:
            data = release.archive_bytes(files, extension)
            self.assertEqual(data, release.archive_bytes(dict(reversed(list(files.items()))), extension))
            archive = self.root / f"test.{extension}"
            archive.write_bytes(data)
            unpacked = release.unpack(archive, self.root / extension)
            self.assertEqual((unpacked / "edgehome-dev").read_bytes(), b"fixture")
            if extension == "tar.gz":
                with tarfile.open(archive) as bundle:
                    self.assertEqual(bundle.getmember("edgehome-dev").mode, 0o755)

    def test_unsafe_archive_paths_rejected_before_writing(self):
        for index, name in enumerate(["../escape", "/escape", "C:/escape", "..\\escape", "a/../escape"]):
            archive = self.root / f"unsafe-{index}.zip"
            archive.write_bytes(release.archive_bytes({name: (b"bad", 0o644)}, "zip"))
            target = self.root / f"unsafe-{index}"
            with self.assertRaises(ValueError):
                release.unpack(archive, target)
            self.assertFalse(target.exists())

    def test_symlinks_in_archive_rejected(self):
        archive = self.root / "links.tar.gz"
        with tarfile.open(archive, "w:gz") as bundle:
            member = tarfile.TarInfo("binary")
            member.type, member.linkname = tarfile.SYMTYPE, "../outside"
            bundle.addfile(member)
        with self.assertRaises(ValueError):
            release.unpack(archive, self.root / "links")

    def test_complete_release_assembles(self):
        self.fixtures()
        self.assemble()
        self.assertEqual(len((self.root / "SHA256SUMS").read_text().splitlines()), len(release.TARGETS))
        manifest = json.loads((self.root / "release-manifest.json").read_text())
        self.assertEqual({p["target"] for p in manifest}, {p["target"] for p in release.TARGETS})
        self.assertIn(self.info["sdk_revision"], (self.root / "release-notes.md").read_text(encoding="utf-8"))

    def test_missing_platform_cannot_publish(self):
        self.fixtures()
        next(self.root.glob("*.zip")).unlink()
        with self.assertRaises(ValueError):
            self.assemble()

    def test_corrupt_archive_cannot_publish(self):
        self.fixtures()
        archive = next(self.root.glob("*.zip"))
        archive.write_bytes(archive.read_bytes() + b"corruption")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            self.assemble()

    def test_wrong_source_sdk_version_target_or_dirty_flag_cannot_publish(self):
        for change in [
            {"source_commit": "b" * 40}, {"sdk_revision": "b" * 40},
            {"version": "999.0.0"}, {"target": "x86_64-unknown-linux-gnu"},
            {"source_dirty": True}, {"release_eligible": False},
        ]:
            with self.subTest(change=change):
                self.fixtures()
                self.mutate_manifest(**change)
                with self.assertRaises(ValueError):
                    self.assemble()

    def test_internal_tampering_fails_even_with_recomputed_archive_checksum(self):
        self.fixtures()
        self.mutate_manifest(files={"edgehome-dev": "0" * 64})
        with self.assertRaisesRegex(ValueError, "inventory or SHA-256"):
            self.assemble()

    def test_extra_nested_manifest_is_not_exempt_from_inventory(self):
        files = {"nested/release.json": (b"unlisted file", 0o644),
                 "release.json": (b'{"files": {}}', 0o644)}
        archive = self.root / "extra.zip"
        archive.write_bytes(release.archive_bytes(files, "zip"))
        unpacked = release.unpack(archive, self.root / "extra")
        with self.assertRaises(ValueError):
            release.verify_package(unpacked)

    def test_no_overwrite_of_existing_extraction_directory(self):
        archive = self.root / "test.zip"
        archive.write_bytes(release.archive_bytes({"original": (b"replace", 0o644)}, "zip"))
        target = self.root / "existing"
        target.mkdir()
        (target / "original").write_bytes(b"keep")
        with self.assertRaises(FileExistsError):
            release.unpack(archive, target)
        self.assertEqual((target / "original").read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
