# Sileo Repository for relayserver — Design

Date: 2026-11-03
Status: Draft (pending approval)

## Intent and why

This repo (a fork of `OpenBubbles/relayserver`) produces a jailbreak tweak —
`dev.copper.relayserver`, a LaunchDaemon that runs the Rust relay server on iOS.
Users install it through Sileo. Today the only way to get a build is to build
`.deb` files locally and sideload them. We want a hosted **Sileo repository**
(users add a URL once in Sileo, then get every version, including updates)
served from **GitHub Pages** at:

    https://jakecrowley.github.io/relayserver/

The repo builds and publishes itself: pushing a release tag is the whole
release process.

## Goals

- A valid Sileo/apt repository at the Pages URL above, containing both
  architecture variants of the package:
  - rootful: `iphoneos-arm` (e.g. `dev.copper.relayserver_0.0.3-16+debug_iphoneos-arm.deb`)
  - rootless: `iphoneos-arm64` (e.g. `dev.copper.relayserver_0.0.3-17+debug_iphoneos-arm64.deb`)
- GPG-signed `Release` metadata so Sileo shows the repo as verified
  (no unsigned-repo warning).
- Fully hands-off publishing: a GitHub Actions workflow builds both `.deb`
  files with Theos in CI and deploys the repo on every release tag (`v*`).
- Repo works with both Sileo and other modern jailbreak package managers
  (Zebra, etc. follow the same apt conventions).

## Non-goals

- No repo web front-end beyond the raw repo files (a human-readable landing
  page is out of scope; GitHub Pages will render a trivial index or nothing).
- No support for multiple packages — this repo carries exactly one package,
  `dev.copper.relayserver`.
- No upstream branding changes: the package `control` file stays exactly as
  it is today (Name `relay-package`, Maintainer `Tae Hagen`). This is a
  deliberate user decision.
- No auto-incrementing version logic; versions come from the tag.

## Anti-goals (failure modes)

- A repo that breaks existing installs when it is regenerated (never delete a
  published `.deb` from the Pages branch; only add).
- CI builds that don't reproduce what a local `make both` in
  `relay-package/` produces (the CI recipe must mirror the Makefile, not
  reinvent packaging).

## Constraints

- **Toolchain**: the Rust binary builds with a Linux-hosted Theos iOS
  toolchain (`cargo build --release --target aarch64-apple-ios`, linker =
  Theos clang, SDK = iPhoneOS16.5). CI installs Theos (git clone), the
  cross-toolchain bundle, and the SDK from public sources with pinned
  versions, following the proven recipe in
  `~/Documents/ValidationRelay/scripts/setup_toolchain.sh`:
  - Theos: `git clone --recursive https://github.com/theos/theos.git`
  - Toolchain bundle (contains `clang`, `ar`, `ldid`):
    `https://github.com/kabiroberai/swift-toolchain-linux/releases/download/v2.3.0/swift-5.8-ubuntu22.04.tar.xz`
    (extracted into `$THEOS/toolchain/`; the Swift compiler it bundles is
    unused here)
  - SDK: clone `https://github.com/theos/sdks.git`, copy `iPhoneOS16.5.sdk`
    into `$THEOS/sdks/`
  - The bundle targets Ubuntu 22.04; GitHub Actions `ubuntu-22.04` runners
    match (or newer runners with the symlink workarounds from that script
    for `libedit`/`libncurses`). Rust version pinned to the local one
    (rustc 1.98) via `rustup`.
- **Hardcoded paths**: `.cargo/config.toml` and `build.rs` hardcode
  `/home/jake/theos`. They get parameterized to read `$THEOS` with the local
  path as fallback, so the local build is byte-for-byte unchanged. (User
  approved editing these files; the `control` file stays untouched.)
- **Versioning**: the git tag becomes the package version. CI passes
  `PACKAGE_VERSION=<tag without v>` to Theos, which overrides the hardcoded
  `Version:` in `control` (Theos honors the variable on the command line), and
  clears `PACKAGE_BUILDNAME` so no `+debug` suffix appears: tag `v0.0.4`
  publishes `dev.copper.relayserver_0.0.4_iphoneos-arm.deb` and
  `..._0.0.4_iphoneos-arm64.deb`. This compares as an upgrade over existing
  sideloaded installs (`0.0.3-16+debug`), satisfying the anti-goal about
  breaking existing installs. Existing local tags are unprefixed (`0.0.1`),
  but release tags are `v*`.
- **Two packaging schemes**: Theos builds rootful and rootless in one pass
  via `make both` (rootless = `THEOS_PACKAGE_SCHEME=rootless`). Both .debs
  must land in the repo.
- **Pages**: `github.io` serves the branch root; repo metadata must be at
  the URL root (`/Release`, `/Packages.gz`), with `.deb` files anywhere
  below it.
- **Secrets**: the GPG private key is stored as a GitHub Actions secret
  (`GPG_PRIVATE_KEY` + `GPG_PASSPHRASE` if set). The user keeps a backup of
  the key outside GitHub — losing it means re-signing everything or
  starting a new key.
- Control metadata (`Name`, `Maintainer`) is left as upstream has it.

## Approaches considered

> Decisions below supersede an earlier draft: the user picked "start fresh"
> over extending the existing `sileo-repo` branch, chose clean tag-derived
> versions, approved parameterizing the hardcoded build paths, and pointed at
> `~/Documents/ValidationRelay/scripts/setup_toolchain.sh` as the proven
> toolchain recipe (public downloads, not a private release artifact).

1. **Full CI build and publish (chosen)** — a workflow on `v*` tags installs
   Theos + SDK + Rust, runs the existing Makefile packaging, assembles apt
   repo metadata, signs it, and deploys to the Pages branch. *Trade-off:*
   most setup up front (CI container needs Theos/SDK/Rust), but publishing is
   one command (`git tag vX && git push --tags`) and builds become
   reproducible and visible to anyone.
2. **Local builds + CI publish** — user builds locally; a script/Action
   regenerates metadata on push. *Trade-off:* no CI toolchain setup, but the
   user stays the build machine and every publish is manual. Rejected: user
   chose full automation.
3. **Manual everything** — a local script generates the repo; user copies
   files to Pages. *Trade-off:* simplest to set up, but every release needs
   several manual steps and signing is easy to get wrong. Rejected.

## How it gets built (decided)

### Repo layout on the Pages branch (standard apt pool layout)

    Release                      # apt release file, GPG-signed
    Release.gpg                  # detached signature
    Packages                     # plain index (optional but harmless)
    Packages.gz                  # gzipped package index (what Sileo reads)
    pool/main/
      dev.copper.relayserver_*_iphoneos-arm.deb
      dev.copper.relayserver_*_iphoneos-arm64.deb

`Release` lists `Packages`/`Packages.gz` checksums; `Packages.gz` lists each
`.deb`'s metadata (parsed from the deb's control file by `dpkg-scanpackages`
or equivalent logic in a script).

### Workflow (`.github/workflows/repo.yml`)

Trigger: push of tags matching `v*`, cut from `master` (workflow file lives
on `master`; tag-triggered runs execute the file at the tag's commit).

1. Checkout, install Theos (git clone to `$THEOS`), download the pinned
   toolchain bundle into `$THEOS/toolchain/`, install the iPhoneOS16.5 SDK
   from theos/sdks, install Rust 1.98 with the `aarch64-apple-ios` target.
   Cache all of this between runs keyed on the pinned versions.
2. Build both variants: parameterized build files let cargo use `$THEOS`;
   run the same `make both` invocation the local Makefile supports (rootful
   then rootless, with `PACKAGE_VERSION` overridden), i.e. replicate
   `relay-package/Makefile` behavior inside the runner with `THEOS` set.
   Collect the two `.deb` files.
3. Build the repo additively (the design supersedes the `sileo-repo` branch;
   that branch's generator is not used): fetch prior `gh-pages` state (if
   any), add the new `.deb`s under `pool/main/` — never removing old ones,
   per the anti-goal — then assemble apt repo metadata (`Packages.gz`,
   `Release`).
4. Sign `Release` with the GPG secret key. If `GPG_PRIVATE_KEY` is missing,
   the workflow fails loudly — signing is mandatory by design, so a silent
   unsigned publish can never happen.
5. Deploy the repo to the Pages branch (`gh-pages`) — e.g. via
   `peaceiris/actions-gh-pages` or a git push — and Pages picks it up.

The pool layout: `.deb` files live under `pool/main/`, `Packages.gz` and
`Release` at the root — the standard apt pool layout. `Release` lists
`Packages`/`Packages.gz` checksums; the package index lists each `.deb`'s
metadata (parsed from the deb's control file by `dpkg-scanpackages` or
equivalent logic in a script).

### One-time setup (out of CI)

- Generate a GPG key locally (`gpg --full-generate-key`), back it up.
- `gh secret set GPG_PRIVATE_KEY < key.asc` (and passphrase secret if the
  key has one).
- Enable GitHub Pages on the `gh-pages` branch for this repo.

### What the builder decides

- Exact CI container image (Debian/Ubuntu runner + Theos install steps vs.
  a prebuilt Docker image with Theos).
- Exact repo-metadata generation method (`dpkg-scanpackages`/`apt-ftparticle`
  vs. a small Python script that reads deb control files).
- Whether to also publish a plain `Packages` file and `Icons` support.

## What's left to the builder

- CI workflow file(s) and any helper scripts (repo metadata generation,
  signing).
- The one-time GPG setup instructions as a doc the user follows.
- A `README` note documenting "add this URL to Sileo".
- Verification: after the first tag is pushed, confirm in Sileo that the repo
  loads, shows `dev.copper.relayserver`, and installs the correct variant
  rootful vs rootless — on-device.
