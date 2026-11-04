"""Tests for repo/build_repo.py — apt repo generator (pool, Packages, Release).

Fixture debs are built in pure Python + `ar` (binutils): members are
debian-binary, control.tar.gz (containing ./control), data.tar.gz (empty).
"""

import gzip
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from io import BytesIO

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build_repo  # noqa: E402


def make_control(package, version, arch="iphoneos-arm", maintainer="Tae Hagen",
                 name="relay-package", description="A background server",
                 installed_size="1024"):
    """The exact control content a fixture deb carries (upstream passthrough)."""
    return (
        f"Package: {package}\n"
        f"Name: {name}\n"
        f"Version: {version}\n"
        f"Architecture: {arch}\n"
        f"Maintainer: {maintainer}\n"
        f"Installed-Size: {installed_size}\n"
        f"Description: {description}\n"
    )


def make_deb(path, package, version, arch="iphoneos-arm", control=None):
    """Build a minimal .deb at path using only stdlib + `ar` (binutils).

    Members, in dpkg-required order: debian-binary, control.tar.gz, data.tar.gz.
    """
    control = control or make_control(package, version, arch)
    tmpdir = tempfile.mkdtemp(prefix="debtest")
    try:
        cbuf = BytesIO()
        with tarfile.open(fileobj=cbuf, mode="w") as ctar:
            data = control.encode()
            info = tarfile.TarInfo("./control")
            info.size = len(data)
            ctar.addfile(info, BytesIO(data))
        control_member = os.path.join(tmpdir, "control.tar.gz")
        with open(control_member, "wb") as f:
            f.write(cbuf.getvalue())

        dbuf = BytesIO()
        with tarfile.open(fileobj=dbuf, mode="w") as dtar:
            pass
        data_member = os.path.join(tmpdir, "data.tar.gz")
        with open(data_member, "wb") as f:
            f.write(dbuf.getvalue())

        bb = os.path.join(tmpdir, "debian-binary")
        with open(bb, "w") as f:
            f.write("2.0\n")

        deb = os.path.join(tmpdir, f"{package}_{version}_{arch}.deb")
        # ar 'qc' = create archive with members in the given order
        for m in (bb, control_member, data_member):
            subprocess.run(["ar", "qc", deb, m], check=True)
        shutil.copy(deb, path)
        return path
    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def parse_index(text):
    """Split a Packages file into dict stanzas."""
    stanzas = []
    current = {}
    for line in text.splitlines():
        if line.strip():
            k, _, v = line.partition(": ")
            current[k] = v
        else:
            if current:
                stanzas.append(current)
                current = {}
    if current:
        stanzas.append(current)
    return stanzas


class TestRepoGenerator(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="repotest")
        self.packages = os.path.join(self.tmp, "pkgs")
        self.out = os.path.join(self.tmp, "out")
        os.makedirs(self.packages)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_package_stanzas_and_hashes(self):
        deb = make_deb(os.path.join(self.packages, "a.deb"),
                       "dev.copper.relayserver", "0.0.4")
        build_repo.build(self.packages, self.out)
        text = open(os.path.join(self.out, "Packages")).read()
        stanzas = parse_index(text)
        self.assertEqual(len(stanzas), 1)
        s = stanzas[0]
        self.assertEqual(s["Package"], "dev.copper.relayserver")
        self.assertEqual(s["Version"], "0.0.4")
        self.assertEqual(s["Architecture"], "iphoneos-arm")
        self.assertEqual(s["Description"], "A background server")
        self.assertEqual(s["Maintainer"], "Tae Hagen")
        self.assertEqual(s["Installed-Size"], "1024")
        deb_bytes = open(deb, "rb").read()
        self.assertEqual(s["SHA256"], hashlib.sha256(deb_bytes).hexdigest())
        self.assertEqual(s["MD5sum"], hashlib.md5(deb_bytes).hexdigest())
        self.assertEqual(s["SHA1"], hashlib.sha1(deb_bytes).hexdigest())

    def test_filename_field_matches_pool_path(self):
        debname = "dev.copper.relayserver_0.0.4_iphoneos-arm.deb"
        make_deb(os.path.join(self.packages, debname),
                 "dev.copper.relayserver", "0.0.4")
        build_repo.build(self.packages, self.out)
        stanzas = parse_index(open(os.path.join(self.out, "Packages")).read())
        self.assertEqual(len(stanzas), 1)
        s = stanzas[0]
        self.assertEqual(s["Filename"], f"pool/main/{debname}")
        self.assertTrue(
            os.path.isfile(os.path.join(self.out, s["Filename"])),
            "pool file must exist at the exact Filename path")

    def test_packages_gz_matches_packages(self):
        make_deb(os.path.join(self.packages, "a.deb"),
                 "dev.copper.relayserver", "0.0.4")
        build_repo.build(self.packages, self.out)
        plain = open(os.path.join(self.out, "Packages"), "rb").read()
        gz = gzip.decompress(
            open(os.path.join(self.out, "Packages.gz"), "rb").read())
        self.assertEqual(plain, gz)

    def test_release_lists_index_checksums(self):
        make_deb(os.path.join(self.packages, "a.deb"),
                 "dev.copper.relayserver", "0.0.4")
        build_repo.build(self.packages, self.out)
        rel = open(os.path.join(self.out, "Release")).read()
        self.assertIn("Origin: relayserver", rel)
        self.assertIn("Label: relayserver", rel)
        self.assertIn("Suite: stable", rel)
        self.assertIn("Codename: stable", rel)
        self.assertIn("Components: main", rel)
        self.assertIn("Architectures: iphoneos-arm iphoneos-arm64", rel)
        m = re.search(r"Date: (.+)", rel)
        self.assertTrue(m and re.match(r".+,.*\d+ .+ \d{4}", m.group(1)),
                        "Date must be RFC2822-ish (Day, DD Mon YYYY ...)")

        self.assertIn("SHA256:", rel)
        pkgs_hash = hashlib.sha256(
            open(os.path.join(self.out, "Packages"), "rb").read()).hexdigest()
        gz_hash = hashlib.sha256(
            open(os.path.join(self.out, "Packages.gz"), "rb").read()).hexdigest()
        for h in (pkgs_hash, gz_hash):
            self.assertIn(h, rel)

    def test_pool_keeps_all_debs(self):
        make_deb(os.path.join(self.packages, "a.deb"),
                 "dev.copper.relayserver", "0.0.3")
        make_deb(os.path.join(self.packages, "b.deb"),
                 "dev.copper.relayserver", "0.0.4")
        build_repo.build(self.packages, self.out)
        pool = os.path.join(self.out, "pool", "main")
        files = os.listdir(pool)
        self.assertEqual(len(files), 2, "additivity: both versions in pool")
        stanzas = parse_index(open(os.path.join(self.out, "Packages")).read())
        self.assertEqual(len(stanzas), 2, "index lists both stanzas")

    def test_control_file_never_touched(self):
        control = make_control("dev.copper.relayserver", "0.0.4",
                               name="relay-package", maintainer="Tae Hagen",
                               description="A background server")
        make_deb(os.path.join(self.packages, "a.deb"),
                 "dev.copper.relayserver", "0.0.4", control=control)
        build_repo.build(self.packages, self.out)
        stanzas = parse_index(open(os.path.join(self.out, "Packages")).read())
        s = stanzas[0]
        self.assertEqual(s["Name"], "relay-package")
        self.assertEqual(s["Maintainer"], "Tae Hagen")
        self.assertEqual(s["Description"], "A background server")


if __name__ == "__main__":
    unittest.main()
