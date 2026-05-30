# Changelog

All notable changes to Blockwright are tracked here.

The project follows a pragmatic early-stage versioning model. Breaking changes are called out explicitly while the runtime and blueprint model are still stabilizing.

## Unreleased

- Added English documentation entrypoint with `README.en.md`.
- Added Web UI language switching between Chinese and English with browser-local persistence.
- Added open-source project files: MIT license, code of conduct, support guide, changelog, and editor configuration.
- Clarified the public license status in the Chinese README and security policy.

## 0.1.x

- Rust/Axum controller with local web UI, health checks, Minecraft API, robot API, MCP bridge, blueprint store, build store, task queue, and Codex CLI integration.
- Fabric mod for Java Edition, single-player, and LAN-opened worlds.
- Paper plugin for standalone server deployments.
- Relative-coordinate blueprint model with block-state-aware material strings.
- Build-record persistence before Minecraft execution and execution-side verification reports.
- Web chat image upload, voice input, local chat history, and in-flight job resume.
