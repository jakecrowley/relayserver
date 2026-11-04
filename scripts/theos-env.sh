#!/usr/bin/env bash
# Exports the toolchain environment for building relayserver with a
# Linux-hosted Theos iOS toolchain. Source with $THEOS set, or rely on the
# $HOME/theos default (preserves the pre-parameterization local layout).

THEOS="${THEOS:-$HOME/theos}"
export THEOS

SDK="$THEOS/sdks/iPhoneOS16.5.sdk"
TOOLCHAIN_BIN="$THEOS/toolchain/linux/iphone/bin"

# Rust: linker/ar for aarch64-apple-ios (replaces .cargo/config.toml's
# [target.aarch64-apple-ios], which cannot expand env vars).
export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$TOOLCHAIN_BIN/clang"
export CARGO_TARGET_AARCH64_APPLE_IOS_AR="$TOOLCHAIN_BIN/ar"
export CARGO_TARGET_AARCH64_APPLE_IOS_RUSTFLAGS="-C link-arg=-isysroot -C link-arg=$SDK -C link-arg=-F$SDK/System/Library/Frameworks -C link-arg=-L$SDK/usr/lib -C link-arg=-target -C link-arg=arm64-apple-ios14.0"

# cc/Build script for src/c (replaces .cargo/config.toml's [env] section).
export CC_aarch64_apple_ios="$TOOLCHAIN_BIN/clang"
export CXX_aarch64_apple_ios="$TOOLCHAIN_BIN/clang++"
export AR_aarch64_apple_ios="$TOOLCHAIN_BIN/ar"
export CFLAGS_aarch64_apple_ios="-isysroot $SDK -F$SDK/System/Library/Frameworks -target arm64-apple-ios14.0"
export SDKROOT="$SDK"
export IPHONEOS_DEPLOYMENT_TARGET="14.0"
