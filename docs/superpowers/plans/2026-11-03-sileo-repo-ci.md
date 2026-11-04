# Sileo Repo CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pushing a `v*` tag builds both `.deb` variants in GitHub Actions with Theos, assembles a GPG-signed apt/Sileo repository, and publishes it to GitHub Pages at `https://jakecrowley.github.io/relayserver/`.

**Architecture:** A shell env script parameterizes the Theos toolchain paths so cargo can build both locally and in CI. A `build_debs.sh` wrapper invokes the existing `relay-package/Makefile` with a tag-derived `PACKAGE_VERSION`. A stdlib-only Python generator scans `.deb` files into `Packages(.gz)` + `Release` and optionally signs. The workflow installs the pinned public toolchain (per the ValidationRelay recipe), builds, generates, signs, and additively deploys to `gh-pages`.

**Tech Stack:** Theos + `kabiroberai/swift-toolchain-linux` v2.3.0 bundle + iPhoneOS16.5.sdk (public downloads), Rust 1.98.0 (aarch64-apple-ios), bash, Python 3 stdlib (`ar` from binutils for deb parsing), GPG detached signatures, GitHub Actions + GitHub Pages.

**Spec:** `docs/superpowers/specs/2026-11-03-sileo-repo-design.md`

## Global Constraints

- Repo URL root: `https://jakecrowley.github.io/relayserver/`; layout is standard apt pool: `Release`, `Release.gpg`, `Packages`, `Packages.gz` at root, `.deb` files under `pool/main/`.
- Never delete or replace a published `.deb` — publishing is additive only; old versions stay in the pool and in the index (Sileo picks the newest).
- Workflow triggers only on tags matching `v*`; version passed to Theos is the tag without the `v` prefix; `PACKAGE_BUILDNAME` is emptied so no `+debug` suffix (published version is exactly e.g. `0.0.4`).
- The `relay-package/control` file is NOT modified (Name/Maintainer stay as upstream has them).
- Toolchain pins, verbatim: Theos via `git clone --recursive https://github.com/theos/theos.git`; toolchain bundle `https://github.com/kabiroberai/swift-toolchain-linux/releases/download/v2.3.0/swift-5.8-ubuntu22.04.tar.xz` extracted into `$THEOS/toolchain/`; SDK `iPhoneOS16.5.sdk` copied from a `git clone --depth=1 https://github.com/theos/sdks.git`; Rust `1.98.0` with target `aarch64-apple-ios`. Runner OS: `ubuntu-22.04` (matches the bundle).
- GPG signing is mandatory in CI: secrets `GPG_PRIVATE_KEY` (armored private key) and `GPG_PASSPHRASE` (may be empty). If `GPG_PRIVATE_KEY` is unset the workflow fails loudly — never publish unsigned.
- Workflow file lives on `master`; tags are cut from `master`; deploy target is the `gh-pages` branch.
- `relay-package` packaging replicates the existing Makefile (`make both` = rootful `iphoneos-arm` then rootless with `THEOS_PACKAGE_SCHEME=rootless`); don't reinvent packaging.
- Local machine has NO `dpkg-deb`; the generator parses debs with `ar` + Python stdlib only.

## Review Focus

- **Additivity:** a repo regeneration must never drop old `.deb`s — a user on an old version must still be able to download it. Test: `test_pool_keeps_all_debs` (Task 3) pins that the index and pool include every scanned deb across versions.
- **Upgrade comparison:** first published version must compare greater than sideloaded `0.0.3-16+debug` installs. Test: `test_build_debs_names_and_version` (Task 2) pins the exact output filenames `dev.copper.relayserver_<clean tag>_iphoneos-arm{,64}.deb` with no `-N+debug` suffix.
- **Unsigned publish:** a missing GPG secret must stop the pipeline, not degrade to an unsigned repo. Test: `test_require_sign_fails_without_key` (Task 4) pins the fail-loud exit; the workflow step (Task 5) invokes with `--require-sign`.
- **Index/Filename consistency:** every `Filename:` field in `Packages` must be a URL path under the repo root that actually contains that deb (wrong `Filename:` is the classic silent repo breakage). Test: `test_filename_field_matches_pool_path` (Task 3).
- **Toolchain drift:** CI must use the pinned bundle URL/version from Global Constraints — a reviewer checks the workflow's download URL and cache key literal against this section (Task 5 verification step), since drift can't be unit-tested.

---

### Task 1: Parameterize the toolchain env

**Files:**
- Create: `scripts/theos-env.sh`
- Modify: `.cargo/config.toml`
- Modify: `relay-package/Makefile:15-16` (the `before-package::` cargo invocation area)

**Interfaces:**
- Produces: `scripts/theos-env.sh` — sourced with `$THEOS` set (or defaulting to `$HOME/theos`), exports exactly these env vars: `CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$THEOS/toolchain/linux/iphone/bin/clang"`, `CARGO_TARGET_AARCH64_APPLE_IOS_AR="$THEOS/toolchain/linux/iphone/bin/ar"`, `CARGO_TARGET_AARCH64_APPLE_IOS_RUSTFLAGS` (the isysroot/-F/-L/-target link args from the current config, with `$THEOS/sdks/iPhoneOS16.5.sdk` paths), `CC_aarch64_apple_ios`/`CXX_aarch64_apple_ios`/`AR_aarch64_apple_ios` (Theos clang/clang++/ar), `CFLAGS_aarch64_apple_ios` (same isysroot/-F/-target flags), `SDKROOT="$THEOS/sdks/iPhoneOS16.5.sdk"`, `IPHONEOS_DEPLOYMENT_TARGET=14.0`. `build.rs` is left untouched — it already reads `$THEOS` with the `/home/jake/theos` fallback, so the fallback behavior is preserved.

- [ ] **Step 1: Write the failing test**

```bash
# scripts/test_theos_env.sh
#!/usr/bin/env bash
set -euo pipefail
fail=0
check() { if [ "$1" = "$2" ]; then echo "ok: $3"; else echo "FAIL: $3 (got '$1', want '$2')"; fail=1; fi; }
export THEOS=/home/jake/theos
# shellcheck disable=SC1091
source scripts/theos-env.sh
check "${CARGO_TARGET_AARCH64_APPLE_IOS_LINKER:-}" "$THEOS/toolchain/linux/iphone/bin/clang" "linker from THEOS"
check "${CC_aarch64_apple_ios:-}" "$THEOS/toolchain/linux/iphone/bin/clang" "cc from THEOS"
check "${IPHONEOS_DEPLOYMENT_TARGET:-}" "14.0" "deployment target"
# default when THEOS unset
unset THEOS
env -i PATH="$PATH" HOME="$HOME" bash -c 'source scripts/theos-env.sh; test "${CARGO_TARGET_AARCH64_APPLE_IOS_LINKER:-}" = "$HOME/theos/toolchain/linux/iphone/bin/clang" && echo ok: default to HOME/theos' || { echo "FAIL: default"; fail=1; }
[ "$fail" = 0 ] && echo ALL PASS || exit 1
```

- [ ] **Step 2: Run test to verify it fails**

Run: `bash scripts/test_theos_env.sh`
Expected: FAIL (script doesn't exist).

- [ ] **Step 3: Implement `scripts/theos-env.sh` and strip the hardcoded values**

`scripts/theos-env.sh`: `THEOS="${THEOS:-$HOME/theos}"`, then `export` the variables from the Interfaces block, deriving isysroot/-F/-L paths from `$THEOS` (mirror the literal flag values from the current `.cargo/config.toml` `rustflags`/`CFLAGS` entries, substituting `/home/jake/theos` → `$THEOS`). In `.cargo/config.toml`, keep `[build] target = "aarch64-apple-ios"` and delete the `[target.aarch64-apple-ios]` and `[env]` sections (cargo config can't expand env vars; env script replaces them). In `relay-package/Makefile`, immediately before the `cd .. && cargo build...` line in `before-package::`, add `. ../../scripts/theos-env.sh` (the Makefile runs from `relay-package`; note the path is relative to `$(THEOS_PROJECT_DIR)` — use the absolute form `. $(ROOT_DIR)/scripts/theos-env.sh` with `ROOT_DIR := $(shell dirname $@/../..)` computed at the top of the Makefile if the relative source misbehaves under make's shell).

- [ ] **Step 4: Run test to verify it passes**

Run: `bash scripts/test_theos_env.sh`
Expected: ALL PASS.

- [ ] **Step 5: Smoke the real build still works**

Run: `cd relay-package && make -f Makefile before-package 2>&1 | tail -5` — Expected: cargo build.rs runs against `$THEOS` (build script output shows MobileGestalt/CoreFoundation link args resolved; exits 0).
Then: `cd relay-package && make -n package 2>&1 | grep -c "cargo build"` — Expected: at least 1 (Makefile still drives the build).

- [ ] **Step 6: Commit**

```bash
git add scripts/theos-env.sh scripts/test_theos_env.sh .cargo/config.toml relay-package/Makefile
git commit -m "feat: parameterize Theos toolchain paths via env script"
```

---

### Task 2: Build both debs from a tag version

**Files:**
- Create: `scripts/build_debs.sh`
- Create: `scripts/test_build_debs.sh`

**Interfaces:**
- Consumes: `scripts/theos-env.sh` (Task 1), `relay-package/Makefile` `both` target (unmodified semantics).
- Produces: `scripts/build_debs.sh <version>` — e.g. `build_debs.sh 0.0.4` → creates `relay-package/packages/dev.copper.relayserver_0.0.4_iphoneos-arm.deb` and `dev.copper.relayserver_0.0.4_iphoneos-arm64.deb`, exiting nonzero if either is missing. Passes `PACKAGE_VERSION=<version> PACKAGE_BUILDNAME=` to make (command-line assignment suppresses the Theos `-N+buildname` suffix; base version comes from the override, per `theos/makefiles/package.mk` `_THEOS_INTERNAL_PACKAGE_VERSION`).

- [ ] **Step 1: Write the failing test**

```bash
# scripts/test_build_debs.sh — runs build_debs.sh with a throwaway version and
# asserts the produced filenames carry the clean version and both schemes.
#!/usr/bin/env bash
set -euo pipefail
# requires THEOS toolchain (Task 1). Builds for real — slow but the deliverable.
V="0.0.$(date +%s)"; V="0.0.99"
bash scripts/build_debs.sh 0.0.99
ls relay-package/packages/dev.copper.relayserver_0.0.99_iphoneos-arm.deb >/dev/null || { echo FAIL: rootful missing; exit 1; }
ls relay-package/packages/dev.copper.relayserver_0.0.99_iphoneos-arm64.deb >/dev/null || { echo FAIL: rootless missing; exit 1; }
# no build counter or +debug suffix anywhere
if ls relay-package/packages/dev.copper.relayserver_0.0.99-* >/dev/null 2>&1; then echo "FAIL: suffixed variants exist"; exit 1; fi
# version inside the deb control matches
tar -xOf <(ar p relay-package/packages/dev.copper.relayserver_0.0.99_iphoneos-arm.deb control.tar.gz 2>/dev/null || ar p relay-package/packages/dev.copper.relayserver_0.0.99_iphoneos-arm.deb control.tar.zst) 2>/dev/null | tar -xOzf - ./control 2>/dev/null | grep -q "^Version: 0.0.99$" || { echo "FAIL: control Version"; exit 1; }
echo ALL PASS
```

(If `ar p ... control.tar.gz` extraction needs adjusting for the local Theos deb layout — control member may be `control.tar.gz` or `control.tar.xz` — adjust the extraction in the test, not the assertion `^Version: 0.0.99$`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `bash scripts/test_build_debs.sh`
Expected: FAIL (`scripts/build_debs.sh: No such file or directory`).

- [ ] **Step 3: Implement `scripts/build_debs.sh`**

```bash
#!/usr/bin/env bash
# Usage: build_debs.sh <version>  — e.g. build_debs.sh 0.0.4
set -euo pipefail
[ $# -eq 1 ] && [ -n "$1" ] || { echo "usage: $0 <version>" >&2; exit 1; }
VERSION="$1"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export THEOS="${THEOS:-$HOME/theos}"
. "$ROOT_DIR/scripts/theos-env.sh"
cd "$ROOT_DIR/relay-package"
make both PACKAGE_VERSION="$VERSION" PACKAGE_BUILDNAME=
DEBS=(packages/dev.copper.relayserver_${VERSION}_iphoneos-arm.deb packages/dev.copper.relayserver_${VERSION}_iphoneos-arm64.deb)
for d in "${DEBS[@]}"; do [ -f "$d" ] || { echo "missing $d" >&2; exit 1; }; done
echo "OK: ${DEBS[@]}"
```

- [ ] **Step 4: Run test to verify it passes**

Run: `bash scripts/test_build_debs.sh` (a full Theos build — takes minutes; requires the local toolchain)
Expected: ALL PASS, exactly the two files `dev.copper.relayserver_0.0.99_iphoneos-arm{,64}.deb` in `relay-package/packages/`.

- [ ] **Step 5: Commit**

```bash
git add scripts/build_debs.sh scripts/test_build_debs.sh
git commit -m "feat: tag-driven deb build wrapper for both packaging schemes"
```

---

### Task 3: Repo generator — pool, Packages, Release

**Files:**
- Create: `repo/build_repo.py`
- Create: `repo/test_build_repo.py`

**Interfaces:**
- Consumes: `.deb` files in a packages directory (parsed with `ar` + Python stdlib `tarfile`; NO `dpkg-deb`).
- Produces: `build_repo.py --packages-dir DIR --out-dir OUT [--origin S --label S --suite S --codename S] [--require-sign --gpg-key ID]` → in `OUT`: copies every deb into `pool/main/`, writes `Packages`, `Packages.gz`, and `Release` (fields `Origin Label Suite Codename Components: main Architectures: iphoneos-arm iphoneos-arm64 Date` + SHA256/MD5/SHA1 sums of `Packages`/`Packages.gz`). Task 4 adds signing; Task 5's workflow invokes it with `--require-sign`.

- [ ] **Step 1: Write the failing tests**

`repo/test_build_repo.py` (pytest) — fixture builder constructs a minimal deb in pure Python + `ar` subprocess (members: `debian-binary`, `control.tar.gz` containing `./control` with `Package/Version/Architecture/Maintainer/Description`, `data.tar.gz` empty). Key tests:

```python
def test_package_stanzas_and_hashes(): ...      # Packages lists Package/Version/Architecture/Description/Maintainer/Installed-Size/Size/SHA256/MD5/SHA1/Filename for each deb; sums match the deb bytes
def test_filename_field_matches_pool_path(): ... # Filename: pool/main/<debname> and OUT/pool/main/<debname> exists for each stanza
def test_packages_gz_matches_packages(): ...     # gzip -d of Packages.gz == Packages
def test_release_lists_index_checksums(): ...    # Release has SHA256 sums section with entries for Packages and Packages.gz, Components: main, both Architectures
def test_pool_keeps_all_debs(): ...              # with two fixture debs of the same package at versions 0.0.3 and 0.0.4, pool/main contains both and Packages has both stanzas (additivity, newest picked by client)
def test_control_file_never_touched(): ...       # control content read from deb appears verbatim (Name/Maintainer passthrough) — generator never rewrites metadata
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest repo/test_build_repo.py -v`
Expected: FAIL with `ModuleNotFoundError: No module named 'build_repo'` (or collection error for missing file).

- [ ] **Step 3: Implement `build_repo.py`**

Functions: `read_deb_control(path) -> dict` (via `ar p <deb> control.tar.*` piped to `tarfile` — accept `.gz` and `.zst` members; error if neither), `format_stanza(fields) -> str`, `build(packages_dir, out_dir, ...) -> None` copying debs to `out/pool/main/`, writing `Packages` (stanza per deb, `Filename: pool/main/<name>`), gzip to `Packages.gz`, and the `Release` file with `Date` in `Day, DD Mon YYYY HH:MM:SS UTC` format and checksum sections. Signing is NOT in this task.

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest repo/test_build_repo.py -v`
Expected: PASS (all 6).

- [ ] **Step 5: Commit**

```bash
git add repo/build_repo.py repo/test_build_repo.py
git commit -m "feat: stdlib-only apt repo generator (pool, Packages, Release)"
```

---

### Task 4: GPG signing + fail-loud requirement

**Files:**
- Modify: `repo/build_repo.py` (signing function)
- Modify: `repo/test_build_repo.py`

**Interfaces:**
- Consumes: `Release` file from Task 3's `build()`.
- Produces: `sign_release(out_dir, key_id, passphrase: str|None) -> None` writing `Release.gpg` as a detached binary signature of `Release`; and `--require-sign` behavior: when set and no usable GPG secret key is found, exit nonzero with an error naming `GPG_PRIVATE_KEY`. Task 5's workflow passes `--require-sign --gpg-key "$KEY_ID"` after importing the secret.

- [ ] **Step 1: Write the failing tests**

```python
def test_sign_release_creates_release_gpg(): ...   # with a throwaway gpg key generated in the test (gpg --batch --quick-generate-key, GNUPGHOME=tempdir), Release.gpg exists and gpg --verify succeeds against Release
def test_require_sign_fails_without_key(): ...     # empty GNUPGHOME + --require-sign → SystemExit != 0 and error mentions GPG_PRIVATE_KEY; no Release.gpg written
def test_signing_optional_by_default(): ...        # without --require-sign and without key: exits 0, no Release.gpg (local dev path)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `python3 -m pytest repo/test_build_repo.py -k sign -v`
Expected: FAIL (no signing code / no `--require-sign` flag).

- [ ] **Step 3: Implement signing**

`sign_release` runs `gpg --batch --yes --default-key <key_id> --detach-sign --output Release.gpg Release` (add `--passphrase` + `--pinentry-mode loopback` when passphrase non-empty). Key discovery for the `--gpg-key` default: first secret key fingerprint via `gpg --list-secret-keys --with-colons --fingerprint`. `--require-sign`: after signing (or if `gpg`/key unavailable), verify `Release.gpg` exists, else `sys.exit` with the message.

- [ ] **Step 4: Run tests to verify they pass**

Run: `python3 -m pytest repo/test_build_repo.py -v`
Expected: PASS (all 9).

- [ ] **Step 5: Commit**

```bash
git add repo/build_repo.py repo/test_build_repo.py
git commit -m "feat: mandatory-in-CI GPG signing of Release (fail-loud)"
```

---

### Task 5: The publishing workflow

**Files:**
- Create: `.github/workflows/repo.yml`

**Interfaces:**
- Consumes: `scripts/build_debs.sh` (Task 2), `repo/build_repo.py --require-sign` (Tasks 3–4), secret `GPG_PRIVATE_KEY` (+ optional `GPG_PASSPHRASE`), the Global Constraints toolchain pins.
- Produces: a `gh-pages` branch containing the pool + metadata; the Pages URL serves the repo. Workflow is the only writer of `gh-pages`.

- [ ] **Step 1: Write the workflow**

Job `publish` on `runs-on: ubuntu-22.04`, triggered by `push: tags: ['v*']`, `permissions: { contents: write, pages: write, id-token: write }` (contents:write for the gh-pages push). Steps in order:

1. `actions/checkout@v4`.
2. Install apt deps: `build-essential git perl curl rsync zstd xz libxml2-legacy zip binutils python3 python3-pip` (plus the libedit/libncurses symlink workaround from `~/Documents/ValidationRelay/scripts/setup_toolchain.sh` step 4, guarded by file existence).
3. Cache `~/theos` with `actions/cache@v4`, key literal `"theos-swift-toolchain-v2.3.0-iPhoneOS16.5"` (cache key string must match Global Constraints exactly — this is the toolchain-drift tripwire). On miss: clone Theos recursive, download the pinned bundle URL, `tar -xJf` into `$THEOS/toolchain/`, clone theos/sdks depth=1 and copy `iPhoneOS16.5.sdk` into `$THEOS/sdks/`.
4. Install Rust: `rustup toolchain install 1.98.0 --profile minimal --target aarch64-apple-ios` + `rustup default 1.98.0`.
5. Build: `bash scripts/build_debs.sh "${GITHUB_REF_NAME#v}"`.
6. Fetch prior pool additively: if `gh-pages` branch exists (`git ls-remote --heads origin gh-pages`), `git fetch origin gh-pages --depth=1` and copy `pool/main/*.deb` from it into the packages dir before generating (never delete, per Global Constraints).
7. Import the signing key: fail the step (not just warn) if `GPG_PRIVATE_KEY` is empty: `test -n "$GPG_PRIVATE_KEY"`; then `echo "$GPG_PRIVATE_KEY" | gpg --batch --import`; key id from `gpg --list-secret-keys --with-colons --fingerprint | awk -F: '/^fpr:/{print $10; exit}'`.
8. Generate + sign: `python3 repo/build_repo.py --packages-dir relay-package/packages --out-dir out --require-sign --gpg-key "$KEY_ID" --origin relayserver --label relayserver --suite stable --codename stable --passphrase "$GPG_PASSPHRASE"`.
9. Deploy: `peaceiris/actions-gh-pages@v4` with `publish_branch: gh-pages`, `publish_dir: ./out`, `force_orphan: true` is NOT allowed (would violate additivity on non-pool files is fine, but pool must persist — use `keep_files` or the step-6 additive fetch; reviewer verifies pool files are additive), `commit_message: "publish repo ${GITHUB_REF_NAME}"`.

- [ ] **Step 2: Verify the workflow mechanically**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/repo.yml')) && print('yaml ok')"` — Expected: `yaml ok`.
Run: `grep -c "swift-5.8-ubuntu22.04.tar.xz\|iPhoneOS16.5.sdk\|1.98.0" .github/workflows/repo.yml` — Expected: `3` (each pin appears).
Run: `grep -n "require-sign\|GPG_PRIVATE_KEY" .github/workflows/repo.yml` — Expected: both present; the `test -n` guard precedes the import step.
Run: `grep -n "gh-pages" .github/workflows/repo.yml | grep -v orphan` — Expected: fetch + deploy steps reference `gh-pages`; no `force_orphan: true` without an additive rationale comment.

- [ ] **Step 3: Dry-run the pipeline locally without signing (spike, throwaway)**

Run: `GPG_PRIVATE_KEY="" bash -c 'python3 repo/build_repo.py --packages-dir relay-package/packages --out-dir /tmp/opencode/repo-dryrun --origin relayserver --label relayserver --suite stable --codename stable'` then `ls /tmp/opencode/repo-dryrun /tmp/opencode/repo-dryrun/pool/main` — Expected: `Release`, `Packages`, `Packages.gz`, `pool/main/` with the Task 2 debs. (CI runs the same command with `--require-sign`; this spike proves the wiring, not CI.)

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/repo.yml
git commit -m "ci: build and publish signed Sileo repo to GitHub Pages on tags"
```

---

### Task 6: Setup docs + README

**Files:**
- Create: `repo/SETUP.md`
- Create: `README.md` (none exists on master)

**Interfaces:**
- Consumes: Task 5's workflow; Global Constraints one-time setup.
- Produces: user-followable instructions (the one-time GPG + Pages setup the spec requires).

- [ ] **Step 1: Write `repo/SETUP.md`**

Exact content requirements: (1) generate a GPG key once (`gpg --full-generate-key`), export and BACK IT UP outside GitHub (`gpg --export-secret-keys -a ID > gpg-private.asc`); losing it means re-signing everything; (2) `gh secret set GPG_PRIVATE_KEY < gpg-private.asc` and `gh secret set GPG_PASSPHRASE` (only if the key has a passphrase); (3) enable GitHub Pages → Source: `gh-pages` branch (Settings → Pages) — the branch doesn't exist until the first tag; enable after the first run; (4) release flow: `git tag v0.0.4 && git push origin v0.0.4`; (5) verify: the run goes green, `https://jakecrowley.github.io/relayserver/Release` downloads, and in Sileo → Sources → Add → the URL, then install `dev.copper.relayserver` (choose the arm64 variant on rootless jailbreaks).

- [ ] **Step 2: Write the README Sileo section**

`README.md` with a short project line (fork of `OpenBubbles/relayserver`, Rust relay server packaged as `dev.copper.relayserver`) and a **Install via Sileo** section: add `https://jakecrowley.github.io/relayserver/` as a source, then install the package. Keep it under 25 lines total.

- [ ] **Step 3: Verify**

Run: `grep -c "GPG_PRIVATE_KEY\|jakecrowley.github.io/relayserver" repo/SETUP.md README.md` — Expected: at least 2 occurrences total per file (URL in both; secret name in SETUP).
Run: `test ! -f relay-package/control.orig && git diff --stat upstream/master -- relay-package/control` — Expected: no output (control untouched).

- [ ] **Step 4: Commit**

```bash
git add repo/SETUP.md README.md
git commit -m "docs: one-time setup guide and Sileo install instructions"
```

---

## Post-implementation verification (no code changes)

- [ ] Push a real tag (`git tag v0.0.4 && git push origin v0.0.4`) — workflow goes green end-to-end.
- [ ] Check `gh-pages` branch contains `Release`, `Release.gpg`, `Packages.gz`, and `pool/main/` with both debs; a SECOND tag run adds debs without removing the first run's.
- [ ] On-device: Sileo → add the URL → repo shows as verified (no unsigned warning) → install matches the device's scheme (rootless arm64 vs rootful arm) → daemon starts (`/var/mobile/relayserver.log` grows).
