GRADLE ?= gradle
GAME_DIR ?= auto
JAVA21_HOME ?= $(shell (test -d /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home && echo /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home) || /usr/libexec/java_home -v 21 2>/dev/null || true)
GRADLE_JAVA21_ENV = $(if $(JAVA21_HOME),JAVA_HOME="$(JAVA21_HOME)")
.DEFAULT_GOAL := install-java

.PHONY: install install-java run-web test test-controller test-paper test-fabric build-plugins build-fabric build-fabric-universal build-controller-bundle build-controller-bundle-all build-paper coverage coverage-controller

install: install-java

install-java:
	./scripts/install-java-mod.sh "$(GAME_DIR)"

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

build-controller-bundle:
	./scripts/build-controller-bundle.sh --current-platform

build-controller-bundle-all:
	./scripts/build-controller-bundle.sh --all-platforms

build-fabric-universal:
	./scripts/build-java-mod.sh --all-platforms

build-paper:
	cd plugins/paper && $(GRADLE_JAVA21_ENV) $(GRADLE) build

build-plugins: build-fabric build-paper

coverage: coverage-controller

coverage-controller:
	cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80
