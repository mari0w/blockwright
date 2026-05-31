# Contributing Guide

Thank you for your interest in Blockwright. The project is still in an early stage. The current contribution priority is to make the local Minecraft Java Edition and Fabric workflow stable first, then gradually expand planning features and chat entry points.

## Development Boundaries

- Minecraft execution logic belongs in `plugins/fabric` and `plugins/paper`.
- AI planning, blueprint management, task queues, chat tools, and the Codex CLI entry point belong in `apps/controller`.
- Do not put external bots, image analysis, or Codex CLI calls inside the Minecraft plugins.
- Blueprint block coordinates must use relative coordinates.
- Build tasks must save a build record in the controller before sending the same block list to Minecraft.
- The execution side must read world blocks one by one and produce a verification report.
- Player-facing and operator-facing text should keep the existing Chinese copy where the product surface requires it.

## Local Environment

Required tools:

- Rust stable.
- JDK 21.
- Gradle.
- Minecraft 1.21.8 with Fabric Loader.
- Fabric API.
- Optional: `cargo-llvm-cov` for the local coverage gate.
- Optional: Codex CLI.

Start the controller:

```bash
cp .env.example .env
cargo run -p blockwright-controller
```

Install the Fabric mod into the default `.minecraft` directory:

```bash
make
```

Use a custom Java Edition game directory:

```bash
make GAME_DIR=<current Java Edition game directory>
```

## Testing Requirements

New business logic must include tests.

Common checks:

```bash
cargo test --workspace
cd plugins/fabric && gradle test
cd plugins/paper && gradle test
```

Controller coverage gate:

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80
```

Full local test run:

```bash
make test
```

## Code Style

- Run `cargo fmt` before submitting Rust code.
- Keep Java, Kotlin, and Gradle code consistent with the existing plugin structure.
- Complex business branches may use Chinese comments when that matches the surrounding code.
- Do not add heavy abstractions early just to make the project feel more "intelligent"; the first stage prioritizes a verifiable end-to-end workflow.
- Do not commit real tokens, webhooks, client secrets, or local private configuration.
- Player-facing Web copy must keep English and Chinese text in sync. New page text should use the existing language dictionaries first.
- When documentation adds a user entry point, update both the Chinese README and `README.en.md` where possible.

## Pull Request Requirements

PR descriptions should include:

- Purpose of the change.
- Scope of impact.
- Test results.
- Whether the change affects Minecraft execution, blueprint formats, build records, or chat tool credentials.

If the change affects player-visible behavior, include notes or screenshots and update localized copy where relevant.

## Pre-Release Checks

- Use `git status` to confirm that `data/`, `.env`, `config/chat.local.yaml`, and build artifacts were not committed accidentally.
- Controller tests pass.
- Fabric and Paper related tests pass.
- README or docs updates match the behavior changes.
