# sway-miracast: one entry point for building, quality gates and installing.
# Before a commit: make check. First time: make hooks. Install: see INSTALL.md.
CARGO ?= $(HOME)/.cargo/bin/cargo
export PATH := $(HOME)/.cargo/bin:$(PATH)

# Installation. PREFIX/DESTDIR as usual; the user unit goes to the user's systemd config directory.
PREFIX ?= $(HOME)/.local
DESTDIR ?=
BINDIR ?= $(PREFIX)/bin
SHAREDIR ?= $(PREFIX)/share/sway-miracast
UNITDIR ?= $(if $(XDG_CONFIG_HOME),$(XDG_CONFIG_HOME),$(HOME)/.config)/systemd/user

BINARIES := wfd-source wfd-media wfd-tx
# commands: tools/<name>.sh (or .py) is installed as <name> without the extension
SCRIPTS := tools/miracast-menu.sh tools/miracast-status.sh tools/miracast-screen.sh tools/miracast-film.sh \
  tools/miracast-doctor.sh tools/miracast-list.py
COMMANDS := $(basename $(notdir $(SCRIPTS)))
# helpers in share/sway-miracast/lib (audiolat.py: the microphone measurement of miracast-film)
LIBFILES := tools/lib/miracast.sh tools/lib/i18n.sh tools/lib/menu.sh tools/lib/p2p.py tools/audiolat.py
UNIT := sway-miracast-source.service
RELEASE := target/release
REPO := $(CURDIR)

.PHONY: help build test fmt lint check hooks clean install install-dev uninstall

help:
	@echo "make build        - release build (target/release)"
	@echo "make test         - unit tests (no network, no TV)"
	@echo "make fmt          - format Rust and Python"
	@echo "make lint         - fmt --check, clippy -D warnings, pre-commit on the whole repo"
	@echo "make check        - lint + test (before a commit)"
	@echo "make hooks        - install pre-commit (commit and commit message)"
	@echo "make install      - copy binaries, commands and the user unit (PREFIX=$(PREFIX), DESTDIR=$(DESTDIR))"
	@echo "make install-dev  - same layout as symlinks into this repo (target/release, tools/)"
	@echo "make uninstall    - remove what install / install-dev put in place"

build:
	$(CARGO) build --release

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all
	uvx ruff format tools

lint:
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	uvx pre-commit run --all-files

check: lint test

hooks:
	uvx pre-commit install

clean:
	$(CARGO) clean

# --- installation ---------------------------------------------------------------------------------------
# Layout:  $(BINDIR)/{wfd-source,wfd-media,wfd-tx}, $(BINDIR)/miracast-* (symlinks),
#          $(SHAREDIR)/bin/miracast-*, $(SHAREDIR)/lib/*, $(UNITDIR)/$(UNIT).
# The unit is generated from systemd/$(UNIT).in with @BINDIR@ replaced; nothing is enabled or started.
# An existing unit (possibly a symlink into a repo) is removed first so that it is never written through.
define check_release
	@for b in $(BINARIES); do test -x $(RELEASE)/$$b || { echo "missing $(RELEASE)/$$b - run 'make build' first" >&2; exit 1; }; done
endef

define install_unit
	install -d "$(DESTDIR)$(UNITDIR)"
	rm -f "$(DESTDIR)$(UNITDIR)/$(UNIT)"
	sed 's|@BINDIR@|$(BINDIR)|g' systemd/$(UNIT).in >"$(DESTDIR)$(UNITDIR)/$(UNIT)"
	chmod 644 "$(DESTDIR)$(UNITDIR)/$(UNIT)"
endef

define next_steps
	@echo
	@echo "Installed. Next steps (nothing was enabled or started):"
	@echo "  systemctl --user daemon-reload"
	@echo "  systemctl --user enable $(UNIT)"
	@echo "  miracast-doctor      # check the environment"
	@echo "  miracast-menu        # pick a TV (it must show Smart View / screen mirroring)"
	@echo "Make sure $(BINDIR) is on PATH. Sway and Waybar snippets: contrib/"
endef

install:
	$(check_release)
	install -d "$(DESTDIR)$(BINDIR)" "$(DESTDIR)$(SHAREDIR)/bin" "$(DESTDIR)$(SHAREDIR)/lib"
	for b in $(BINARIES); do rm -f "$(DESTDIR)$(BINDIR)/$$b"; install -m 755 $(RELEASE)/$$b "$(DESTDIR)$(BINDIR)/$$b"; done
	for src in $(SCRIPTS); do \
	  c=$$(basename $$src); c=$${c%.*}; \
	  rm -f "$(DESTDIR)$(SHAREDIR)/bin/$$c"; \
	  install -m 755 $$src "$(DESTDIR)$(SHAREDIR)/bin/$$c"; \
	  ln -sfr "$(DESTDIR)$(SHAREDIR)/bin/$$c" "$(DESTDIR)$(BINDIR)/$$c"; \
	done
	for f in $(LIBFILES); do rm -f "$(DESTDIR)$(SHAREDIR)/lib/$$(basename $$f)"; install -m 644 $$f "$(DESTDIR)$(SHAREDIR)/lib/$$(basename $$f)"; done
	chmod 755 "$(DESTDIR)$(SHAREDIR)/lib/audiolat.py"
	$(install_unit)
	$(next_steps)

install-dev:
	$(check_release)
	install -d "$(DESTDIR)$(BINDIR)" "$(DESTDIR)$(SHAREDIR)/bin" "$(DESTDIR)$(SHAREDIR)/lib"
	for b in $(BINARIES); do ln -sf "$(REPO)/$(RELEASE)/$$b" "$(DESTDIR)$(BINDIR)/$$b"; done
	for src in $(SCRIPTS); do \
	  c=$$(basename $$src); c=$${c%.*}; \
	  ln -sf "$(REPO)/$$src" "$(DESTDIR)$(SHAREDIR)/bin/$$c"; \
	  ln -sfr "$(DESTDIR)$(SHAREDIR)/bin/$$c" "$(DESTDIR)$(BINDIR)/$$c"; \
	done
	for f in $(LIBFILES); do ln -sf "$(REPO)/$$f" "$(DESTDIR)$(SHAREDIR)/lib/$$(basename $$f)"; done
	$(install_unit)
	$(next_steps)

uninstall:
	for b in $(BINARIES) $(COMMANDS); do rm -f "$(DESTDIR)$(BINDIR)/$$b"; done
	for c in $(COMMANDS); do rm -f "$(DESTDIR)$(SHAREDIR)/bin/$$c"; done
	for f in $(LIBFILES); do rm -f "$(DESTDIR)$(SHAREDIR)/lib/$$(basename $$f)"; done
	-rmdir "$(DESTDIR)$(SHAREDIR)/bin" "$(DESTDIR)$(SHAREDIR)/lib" "$(DESTDIR)$(SHAREDIR)"
	rm -f "$(DESTDIR)$(UNITDIR)/$(UNIT)"
	@echo "Removed. If the unit was enabled: systemctl --user disable $(UNIT); systemctl --user daemon-reload"
	@echo "Settings and state were left alone: ~/.config/sway-miracast, ~/.local/state/sway-miracast"
