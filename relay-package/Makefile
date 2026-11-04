TARGET := iphone:clang:16.5:14.0

ROOT_DIR := $(shell cd $(patsubst %/,%,$(dir $(abspath $(firstword $(MAKEFILE_LIST)))))/.. && pwd)

include $(THEOS)/makefiles/common.mk

include $(THEOS_MAKE_PATH)/null.mk

clean::
	rm -rf $(THEOS_STAGING_DIR) $(_THEOS_STAGING_TMP)

before-package::
	. "$(ROOT_DIR)/scripts/theos-env.sh" && cd .. && cargo build --release --target aarch64-apple-ios
	$(THEOS)/toolchain/linux/iphone/bin/ldid -S../Entitlements.xml ../target/aarch64-apple-ios/release/relayserver
	rm -rf $(THEOS_STAGING_DIR)/Library $(THEOS_STAGING_DIR)/usr $(THEOS_STAGING_DIR)/var $(_THEOS_STAGING_TMP)
	if [ -n "$(THEOS_PACKAGE_INSTALL_PREFIX)" ]; then mkdir -p $(_THEOS_SCHEME_STAGE); fi
	cp -f layout/DEBIAN/preinst layout/DEBIAN/postinst layout/DEBIAN/prerm $(THEOS_STAGING_DIR)/DEBIAN/
	chmod 755 $(THEOS_STAGING_DIR)/DEBIAN/preinst $(THEOS_STAGING_DIR)/DEBIAN/postinst $(THEOS_STAGING_DIR)/DEBIAN/prerm
	mkdir -p $(THEOS_STAGING_DIR)/usr/local/bin
	cp ../target/aarch64-apple-ios/release/relayserver $(THEOS_STAGING_DIR)/usr/local/bin/relayserver
	chmod 755 $(THEOS_STAGING_DIR)/usr/local/bin/relayserver
	mkdir -p $(THEOS_STAGING_DIR)/Library/LaunchDaemons
	python3 -c 'import plistlib; p = "$(THEOS_PACKAGE_INSTALL_PREFIX)/usr/local/bin/relayserver"; plistlib.dump({"ExecuteAllowed": True, "KeepAlive": True, "RunAtLoad": True, "Label": "dev.copper.relayserver", "Program": p, "ProgramArguments": [p], "WorkingDirectory": "/var/mobile", "StandardOutPath": "/var/mobile/relayserver.log", "StandardErrorPath": "/var/mobile/relayserver.log"}, open("$(THEOS_STAGING_DIR)/Library/LaunchDaemons/dev.copper.relayserver.plist", "wb"), fmt=plistlib.FMT_BINARY)'
	chmod 644 $(THEOS_STAGING_DIR)/Library/LaunchDaemons/dev.copper.relayserver.plist
	sed -i 's/^Architecture:.*/Architecture: $(THEOS_PACKAGE_ARCH)/' $(THEOS_STAGING_DIR)/DEBIAN/control

both:
	$(MAKE) _THEOS_TOP_INVOCATION_DONE= clean
	$(MAKE) _THEOS_TOP_INVOCATION_DONE= package
	$(MAKE) _THEOS_TOP_INVOCATION_DONE= clean
	$(MAKE) _THEOS_TOP_INVOCATION_DONE= package THEOS_PACKAGE_SCHEME=rootless
