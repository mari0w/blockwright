# Blockwright

**Talk to your Minecraft world. Let it answer with action.**

Blockwright is an in-game assistant for Minecraft Java Edition that turns natural language into real gameplay actions. Use `/bw` in chat to ask for items, run commands, inspect the world around you, change time or weather, and create or edit builds while you play.

Instead of stopping to look up command syntax or switching between tools, describe what you want in plain language and let Blockwright handle the next step inside your world.

## What You Can Do

- Ask for items and equipment
- Change time, weather, game mode, or other command-driven gameplay details
- Build simple structures from natural-language requests
- Edit existing builds after scanning the nearby world
- Check player and world context before taking action
- Use a local web UI to configure the assistant and model provider
- Keep building tasks tracked with blueprints, build records, and execution checks

## Example Requests

```text
/bw give me a diamond sword
/bw make it daytime
/bw clear the weather
/bw build me a small wooden cabin
/bw replace the windows of this house with blue glass
/bw give me night vision
```

## Why Blockwright?

Minecraft commands are powerful, but they are not always easy to remember while you are playing. Blockwright makes that power feel more natural: you ask for the outcome, and the assistant turns it into a controlled in-game action.

It is especially useful for:

- Players who want faster creative building
- Local worlds and LAN sessions
- Testing command-driven ideas without memorizing every command
- Experimenting with AI-assisted building and world interaction
- Turning rough ideas into playable Minecraft structures

## Built for Java Edition

Blockwright is designed for Minecraft Java Edition with Fabric as the main installation path for local single-player and LAN-opened worlds. Paper support is kept for standalone server setups.

The Fabric mod connects your game to a local controller. The controller handles assistant planning, configuration, blueprints, task queues, and build records, while the Minecraft-side mod performs the actual world actions.

## Current Status

Blockwright is early, but already usable for local experimentation. The core loop is in place: ask in game, let the assistant plan, send controlled actions to Minecraft, and track build results.

Expect rapid iteration as more building workflows, world-reading tools, and assistant behaviors are refined.

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
