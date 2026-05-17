GRADLE ?= gradle

.PHONY: test test-controller test-paper coverage coverage-controller

test: test-controller test-paper

test-controller:
	cargo test --workspace

test-paper:
	cd plugins/paper && $(GRADLE) test

coverage: coverage-controller

coverage-controller:
	cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80
