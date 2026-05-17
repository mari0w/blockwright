GRADLE ?= gradle
HMCL_DIR ?= $(HOME)/.minecraft
.DEFAULT_GOAL := install-hmcl

.PHONY: install install-hmcl test test-controller test-paper test-fabric build-fabric coverage coverage-controller

install: install-hmcl

install-hmcl:
	./scripts/install-hmcl-mod.sh "$(HMCL_DIR)"

test: test-controller test-paper test-fabric

test-controller:
	cargo test --workspace

test-paper:
	cd plugins/paper && $(GRADLE) test

test-fabric:
	cd plugins/fabric && $(GRADLE) test

build-fabric:
	cd plugins/fabric && $(GRADLE) build

coverage: coverage-controller

coverage-controller:
	cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80
