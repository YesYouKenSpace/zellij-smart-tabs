.PHONY: build build-dev test lint install clean dev dev-reload clean-cache

PLUGIN_NAME = zellij-smart-tabs
WASM_TARGET = wasm32-wasip1
INSTALL_DIR = $(HOME)/.config/zellij/plugins
WASM_DEV = target/$(WASM_TARGET)/debug/$(PLUGIN_NAME).wasm
WASM_RELEASE = target/$(WASM_TARGET)/release/$(PLUGIN_NAME).wasm

build:
	cargo build --release

build-dev:
	cargo build

dev: clean-cache build-dev
	zellij -n dev-layout.kdl --session smart-tabs-dev

test:
	cargo test --target $$(rustc -vV | grep host | awk '{print $$2}')

lint:
	cargo clippy --target $$(rustc -vV | grep host | awk '{print $$2}') -- -D warnings

test-all: test lint build

install: build clean-cache
	mkdir -p $(INSTALL_DIR)
	cp target/$(WASM_TARGET)/release/$(PLUGIN_NAME).wasm $(INSTALL_DIR)/

clean:
	cargo clean

clean-cache:
	rm -rf ~/.cache/zellij
