# Blockwright: production-oriented AI control for Minecraft Java

Blockwright is an open-source AI control layer for **Minecraft Java Edition**.

The product is built around a simple operating model: players and operators describe intent in natural language, and the assistant turns that intent into controlled Minecraft operations through the controller and the Fabric mod.

Supported entrances:

- in-game `/bw` requests
- local Web text chat
- Web voice input
- Matrix / Element rooms
- DingTalk bot messages
- local commands or scripts for custom integrations

Supported request patterns:

```text
/bw give me a diamond sword
/bw scan what I am looking at
/bw set the time to day and stop the rain
/bw build a wooden cabin with windows, a bed, lights, and a reachable entrance
/bw replace the wall in front of me with stone bricks
```

This is not just item delivery or a building prompt. Blockwright is designed as a Minecraft AI assistant that can read context, operate supported commands, place and edit blocks, save records, and report what actually happened.

The execution loop is:

```text
natural language request
-> model-backed planning
-> structured tool call
-> Minecraft-side execution
-> build record / verification report
```

For building, Blockwright places blocks through the server/world API. It does not simulate a player moving a mouse, opening inventory slots, and right-clicking blocks. Blueprints use relative coordinates, build records are saved by the controller, and the Minecraft execution side can read back world state after placement.

Supported model backends are configured from the Web settings page:

- Codex CLI
- OpenAI
- DeepSeek
- Doubao
- Gemini

The main installation path is Fabric for Minecraft Java Edition 1.21.x, including single-player saves and LAN-opened worlds. Paper support remains available for standalone server deployments.

The main operating scenarios are local worlds, LAN worlds, creative-mode build iteration, world-aware assistant actions, Web or voice control, and chat-tool entrances for Minecraft operations.

The engineering focus is reliability: safer existing-world edits, clearer build selection, stronger Minecraft-specific planning around doors, beds, lighting, paths, and block states, and more first-class tools for operations that should not live only in prompt text.

Blockwright is MIT licensed and open source.

GitHub:

https://github.com/mari0w/blockwright
