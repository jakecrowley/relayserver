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
