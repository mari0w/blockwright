# Blockwright

**AI control for Minecraft Java Edition. Natural language in, verified world actions out.**

Blockwright is a production-oriented Minecraft AI assistant for Java Edition. Use `/bw`, the local Web console, voice input, or connected chat tools to turn natural-language requests into controlled Minecraft operations.

The Fabric mod connects the game to a local controller. The controller handles model configuration, planning, tool selection, build records, task queues, and verification reports. The Minecraft-side mod performs the actual world actions through server-side APIs.

## What You Can Do

- Read player and world context, including inventory, held items, and nearby blocks
- Run supported Minecraft command operations such as items, time, weather, effects, and mode changes
- Build and edit structures through blueprint-backed placement
- Save build records before execution and verify placed blocks afterward
- Configure model backends from the local Web console
- Route requests from `/bw`, Web text, Web voice, Matrix/DingTalk, or local scripts

## Supported Request Patterns

```text
/bw give me a diamond sword
/bw make it daytime
/bw clear the weather
/bw build a wooden cabin with windows, a bed, lights, and a reachable entrance
/bw replace the windows of this house with blue glass
/bw give me night vision
```

## Why Blockwright?

Minecraft commands and world edits are powerful, but they are not always ergonomic while players are building or operating a world. Blockwright provides a controlled assistant layer: the user describes the desired outcome, the model selects supported tools, and Minecraft executes the action through the mod.

Primary operating scenarios:

- Players who want an AI assistant that can operate Minecraft, not only answer questions
- Operators who want Web, voice, or chat-tool entrances for Minecraft actions
- Local worlds and LAN sessions
- Creative-mode build iteration with saved records
- Command-heavy operations that should stay conversational but auditable
- World-aware structure editing and verification

## Built for Java Edition

Blockwright is designed for Minecraft Java Edition with Fabric as the main installation path for local single-player and LAN-opened worlds. Paper support is kept for standalone server setups.

The Fabric mod connects your game to a local controller. The controller handles assistant planning, configuration, blueprints, task queues, and build records, while the Minecraft-side mod performs the actual world actions.

## Operational Profile

Blockwright ships the full assistant execution loop: request intake, model-backed planning, structured tool calls, Minecraft-side execution, build records, and verification reporting.

The main installation path is Fabric for Java Edition 1.21.x, including single-player saves and LAN-opened worlds. Paper support is available for standalone server setups.

## Requirements

- Minecraft Java Edition 1.21.8
- Fabric Loader
- Fabric API
- Blockwright Fabric mod
- A configured assistant provider, such as Codex CLI or a supported API provider

## Quick Start

After installing the mod, use `/bw` in Minecraft chat:

```text
/bw give me a diamond sword
```

Use `/bw web` to open the local web UI address, then configure the assistant provider from the settings page.

## Project Links

- GitHub: https://github.com/mari0w/blockwright
- Documentation and setup notes are included in the project repository.
