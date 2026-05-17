GRADLE ?= gradle

.PHONY: test test-controller test-paper test-fabric build-fabric coverage coverage-controller

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
