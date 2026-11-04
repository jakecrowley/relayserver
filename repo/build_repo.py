#!/usr/bin/env python3
"""Sileo/apt repo generator: pool, Packages(.gz), Release.

Stdlib-only (plus `ar` from binutils for deb member extraction). Scans a
packages directory of .deb files, copies them into pool/main/, and writes
the package index and Release metadata that Sileo and other apt clients read.

Signing lives in a separate module-level entry point added alongside; this
module stays CLI-invocable:

    python3 build_repo.py --packages-dir DIR --out-dir OUT
                          [--origin S] [--label S] [--suite S] [--codename S]
"""

import argparse
import gzip
import hashlib
import os
import shutil
import subprocess
import sys
import tarfile
from datetime import datetime, timezone
from io import BytesIO

POOL_SUBDIR = "pool/main"
INDEX_ARCHES = "iphoneos-arm iphoneos-arm64"


def first_secret_key_fingerprint(gnupghome=None):
    """Fingerprint of the first GPG secret key, or None if none exists."""
    env = dict(os.environ)
    if gnupghome:
        env["GNUPGHOME"] = gnupghome
    out = subprocess.run(["gpg", "--batch", "--list-secret-keys",
                          "--with-colons", "--fingerprint"],
                         env=env, capture_output=True, text=True)
    for line in out.stdout.splitlines():
        if line.startswith("fpr:"):
            return line.split(":")[9]
    return None


def sign_release(out_dir, key_id, passphrase, gnupghome=None, require=False):
    """Write Release.gpg as a detached signature of Release.

    key_id None means auto-discover. With require=True, a missing gpg binary
    or secret key exits nonzero with an error naming GPG_PRIVATE_KEY (the CI
    secret), and no Release.gpg is left behind. With require=False and no
    usable key, signing is skipped silently (local dev path).
    """
    release = os.path.join(out_dir, "Release")
    gpg_out = os.path.join(out_dir, "Release.gpg")
    if os.path.exists(gpg_out):
        os.remove(gpg_out)

    env = dict(os.environ)
    if gnupghome:
        env["GNUPGHOME"] = gnupghome

    gpg = shutil.which("gpg")
    key_id = key_id or first_secret_key_fingerprint(gnupghome)
    if not gpg or not key_id:
        if require:
            print("error: signing required but no usable GPG secret key "
                  "found; set the GPG_PRIVATE_KEY secret (and GPG_PASSPHRASE "
                  "if the key has a passphrase)", file=sys.stderr)
            sys.exit(1)
        return

    cmd = [gpg, "--batch", "--yes", "--default-key", key_id,
           "--detach-sign", "--output", gpg_out, release]
    if passphrase:
        cmd = [gpg, "--batch", "--yes", "--passphrase", passphrase,
               "--pinentry-mode", "loopback", "--default-key", key_id,
               "--detach-sign", "--output", gpg_out, release]
    proc = subprocess.run(cmd, env=env, capture_output=True, text=True)
    if proc.returncode != 0 or not os.path.isfile(gpg_out):
        if require:
            print(f"error: signing failed: {proc.stderr}", file=sys.stderr)
            sys.exit(1)
        return


def read_deb_control(path):
    """Return the control file of a .deb as an ordered field dict.

    Extracts the control tar member (control.tar.gz or control.tar.zst) with
    `ar` and reads ./control via tarfile. Errors if neither member exists.
    """
    out = subprocess.run(["ar", "p", path, "control.tar.gz"],
                         capture_output=True, check=False)
    if out.returncode != 0 or not out.stdout:
        out = subprocess.run(["ar", "p", path, "control.tar.zst"],
                             capture_output=True, check=False)
        if out.returncode != 0 or not out.stdout:
            raise ValueError(f"{path}: no control.tar.gz or control.tar.zst member")
    buf = BytesIO(out.stdout)
    with tarfile.open(fileobj=buf, mode="r:*") as ctar:
        for member in ctar.getmembers():
            if member.name.lstrip("./") == "control":
                data = ctar.extractfile(member).read()
                break
        else:
            raise ValueError(f"{path}: control member not found in control tar")
    fields = {}
    for line in data.decode("utf-8", errors="replace").splitlines():
        if not line.strip():
            continue
        key, sep, value = line.partition(":")
        if sep:
            fields[key.strip()] = value.strip()
    return fields


def format_stanza(fields):
    """Format a Packages index stanza from the deb's control fields."""
    lines = []
    for key, value in fields.items():
        lines.append(f"{key}: {value}")
    return "\n".join(lines)


def file_checksums(path):
    """(size, sha256, md5, sha1) of a file's bytes."""
    data = open(path, "rb").read()
    return (len(data),
            hashlib.sha256(data).hexdigest(),
            hashlib.md5(data).hexdigest(),
            hashlib.sha1(data).hexdigest())


def build(packages_dir, out_dir, origin="relayserver", label="relayserver",
          suite="stable", codename="stable"):
    """Assemble the repo into out_dir: pool/main/*.deb, Packages(.gz), Release."""
    out_pool = os.path.join(out_dir, *POOL_SUBDIR.split("/"))
    os.makedirs(out_pool, exist_ok=True)

    debs = sorted(f for f in os.listdir(packages_dir) if f.endswith(".deb"))
    stanzas = []
    for debname in debs:
        src = os.path.join(packages_dir, debname)
        fields = read_deb_control(src)
        size, sha256, md5, sha1 = file_checksums(src)
        dest = os.path.join(out_pool, debname)
        shutil.copy(src, dest)
        stanza_fields = dict(fields)
        stanza_fields["Installed-Size"] = fields.get("Installed-Size", "")
        stanza_fields["Filename"] = f"{POOL_SUBDIR}/{debname}"
        stanza_fields["Size"] = str(size)
        stanza_fields["SHA256"] = sha256
        stanza_fields["MD5sum"] = md5
        stanza_fields["SHA1"] = sha1
        stanzas.append(stanza_fields)

    # Packages: stanza per deb, blank-line separated; Filename path relative
    # to the repo root so the URL resolves under the Pages domain.
    packages_text = "\n\n".join(format_stanza(s) for s in stanzas)
    if stanzas:
        packages_text += "\n"
    packages_path = os.path.join(out_dir, "Packages")
    with open(packages_path, "w") as f:
        f.write(packages_text)

    with open(packages_path, "rb") as fsrc, gzip.open(
            os.path.join(out_dir, "Packages.gz"), "wb") as fdst:
        shutil.copyfileobj(fsrc, fdst)

    # Release: repo metadata + checksum sections for the index files.
    now = datetime.now(timezone.utc)
    date = now.strftime("%a, %d %b %Y %H:%M:%S %Z")
    lines = [
        f"Origin: {origin}",
        f"Label: {label}",
        f"Suite: {suite}",
        f"Codename: {codename}",
        "Components: main",
        f"Architectures: {INDEX_ARCHES}",
        f"Date: {date}",
        "",
    ]
    for field, fname in (("SHA256", "SHA256"), ("MD5sum", "MD5sum"),
                         ("SHA1", "SHA1")):
        lines.append(f"{field}:")
        for index_name in ("Packages", "Packages.gz"):
            p = os.path.join(out_dir, index_name)
            size, sha256, md5, sha1 = file_checksums(p)
            digest = {"SHA256": sha256, "MD5sum": md5, "SHA1": sha1}[field]
            lines.append(f" {digest} {size} {index_name}")
    release_path = os.path.join(out_dir, "Release")
    with open(release_path, "w") as f:
        f.write("\n".join(lines) + "\n")

    return len(stanzas)


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Build an apt/Sileo repository from .deb files.")
    ap.add_argument("--packages-dir", required=True,
                    help="directory containing .deb files")
    ap.add_argument("--out-dir", required=True,
                    help="repo root to write (pool/, Packages, Packages.gz, Release)")
    ap.add_argument("--origin", default="relayserver")
    ap.add_argument("--label", default="relayserver")
    ap.add_argument("--suite", default="stable")
    ap.add_argument("--codename", default="stable")
    ap.add_argument("--require-sign", action="store_true",
                    help="exit nonzero if Release cannot be signed (CI)")
    ap.add_argument("--gpg-key", default=None,
                    help="signing key fingerprint (default: first secret key)")
    ap.add_argument("--passphrase", default="",
                    help="GPG passphrase if the key has one")
    args = ap.parse_args(argv)

    n = build(args.packages_dir, args.out_dir, origin=args.origin,
              label=args.label, suite=args.suite, codename=args.codename)
    sign_release(args.out_dir, args.gpg_key, args.passphrase,
                 require=args.require_sign)
    print(f"OK: {n} package(s) into {args.out_dir}")


if __name__ == "__main__":
    main()
