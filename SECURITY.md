# Security Policy

Blockwright is currently in early development. It is designed for local Minecraft and trusted LAN use by default, and should not be exposed directly to the public internet.

## Supported Scope

Current security maintenance focuses on:

- Controller HTTP API authentication.
- Chat tool credential protection.
- Minecraft command allowlists.
- Build task and blueprint execution boundaries.
- Preventing real keys, webhooks, client secrets, or tokens from entering the repository.

## Reporting a Security Issue

If you find a security issue, please report it through GitHub Security Advisories or a private maintainer channel before publishing exploit details.

Please include as much of the following as possible:

- Affected version or commit.
- Reproduction steps.
- Potential impact.
- Whether the issue involves credential exposure, remote commands, arbitrary block modification, or unauthorized API access.

## Local Deployment Guidance

- Run the controller only on a trusted local machine or trusted LAN.
- Enable `security.require_token` when accessing it across machines.
- Use a strong random `shared_token`.
- Do not commit `.env`, `config/chat.local.yaml`, `config/secrets/`, or any real credentials to Git.
- Do not enable webhook-only chat entry points for local Minecraft scenarios.
