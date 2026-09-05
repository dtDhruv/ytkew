# Install targets. `cargo install` places only the binary, so the desktop
# entry and icon -- which are what give ytkew a name and picture in a
# launcher or a now-playing panel -- need putting in place separately.

PREFIX ?= $(HOME)/.local
DESTDIR ?=

BIN     := $(DESTDIR)$(PREFIX)/bin
APPS    := $(DESTDIR)$(PREFIX)/share/applications
ICONS   := $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps

.PHONY: build install uninstall check clean

build:
	cargo build --release

install: build
	install -Dm755 target/release/ytkew $(BIN)/ytkew
	install -Dm644 ytkew.desktop $(APPS)/ytkew.desktop
	install -Dm644 assets/ytkew.svg $(ICONS)/ytkew.svg
	-update-desktop-database $(APPS) 2>/dev/null
	-gtk-update-icon-cache -qtf $(DESTDIR)$(PREFIX)/share/icons/hicolor 2>/dev/null
	@echo "installed to $(PREFIX)"

uninstall:
	rm -f $(BIN)/ytkew $(APPS)/ytkew.desktop $(ICONS)/ytkew.svg
	-update-desktop-database $(APPS) 2>/dev/null

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test

clean:
	cargo clean
