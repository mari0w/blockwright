GRADLE ?= gradle
HMCL_DIR ?= auto
JAVA21_HOME ?= $(shell (test -d /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home && echo /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home) || /usr/libexec/java_home -v 21 2>/dev/null || true)
GRADLE_JAVA21_ENV = $(if $(JAVA21_HOME),JAVA_HOME="$(JAVA21_HOME)")
.DEFAULT_GOAL := install-hmcl

.PHONY: install install-hmcl run-web test test-controller test-paper test-fabric build-plugins build-fabric build-paper coverage coverage-controller

install: install-hmcl

install-hmcl:
	./scripts/install-hmcl-mod.sh "$(HMCL_DIR)"

run-web:
	./scripts/run-web.sh

test: test-controller test-paper test-fabric

test-controller:
	cargo test --workspace

test-paper:
	cd plugins/paper && $(GRADLE_JAVA21_ENV) $(GRADLE) test

test-fabric:
	cd plugins/fabric && $(GRADLE_JAVA21_ENV) $(GRADLE) test

build-fabric:
	cd plugins/fabric && $(GRADLE_JAVA21_ENV) $(GRADLE) build

build-paper:
	cd plugins/paper && $(GRADLE_JAVA21_ENV) $(GRADLE) build

build-plugins: build-fabric build-paper

coverage: coverage-controller

coverage-controller:
	cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80
