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
