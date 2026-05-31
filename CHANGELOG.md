# Changelog

All notable changes to Blockwright are tracked here.

The project follows a pragmatic early-stage versioning model. Breaking changes are called out explicitly while the runtime and blueprint model are still stabilizing.

## Unreleased

## 0.1.29 - 2026-05-31

- Added a unified Web chat-tools settings flow for Element/Matrix and DingTalk, including persistent local config, secret-safe environment updates, and Matrix polling startup.
- Reworked AI model selection in the Web setup and settings pages with provider-specific model pickers and clearer first-run configuration controls.
- Added a browser-side clear-chat action that resets saved chat history and restored job status.
- Improved LLM API failure messages so upstream DeepSeek and compatible-provider errors such as insufficient balance or invalid API keys are shown directly instead of being reported as generic configuration failures.
- Added a public GitHub Pages blog, a shareable launch article, and stronger Open Graph/Twitter metadata for docs and project pages.
- Switched contributor and issue templates to English for public project workflows.

## 0.1.28 - 2026-05-31

- Added first-run AI model setup in the Web UI and browser-language-aware UI defaults.
- Added live Minecraft context prefetch for API provider mode so Web/robot requests can read player state and nearby block scans before planning.
- Improved planner prompts, response-language handling, scan-context sampling, and API provider request isolation.
- Simplified `/bw` player commands around the main natural-language entrypoint and `/bw web`.
- Localized Fabric/Paper player-facing messages and passed client language through player state.
- Included nearby scan data in Paper direct Minecraft messages, matching the Fabric request path.
- Improved controlled action execution, placement stats, and visible item-delivery behavior.
- Updated public docs, install guidance, website assets, and project funding metadata.

## 0.1.27 - 2026-05-30

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
