#!/usr/bin/env bash
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
ar p relay-package/packages/dev.copper.relayserver_0.0.99_iphoneos-arm.deb control.tar.gz | tar -xOzf - ./control | grep -q "^Version: 0.0.99$" || { echo "FAIL: control Version"; exit 1; }
echo ALL PASS
