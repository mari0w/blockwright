# Blockwright: open-source AI control for Minecraft Java Edition

Blockwright is an open-source Minecraft Java Edition AI assistant that can turn natural-language requests into controlled in-game actions.

After installing the Fabric mod, players can use `/bw`, a local Web chat page, Web voice input, or connected chat tools. Supported model backends include Codex CLI, OpenAI, DeepSeek, Doubao, and Gemini.

Supported request patterns:

```text
/bw give me a diamond sword
/bw scan what I am looking at
/bw set the time to day and stop the rain
/bw build a wooden cabin with windows, a bed, and lights
/bw replace the wall in front of me with stone bricks
```

The important part is the execution model. Blockwright is not only a chatbot and it is not limited to item delivery or building prompts. It gives the assistant a controlled tool layer for player/world context reads, supported command operations, build and structure-editing workflows, saved blueprints, build records, and verification reports.

The core loop is:

```text
natural language request -> model-backed planning -> structured tool call -> Minecraft execution -> verification/reporting
```

Block placement is done through the server/world API, not by simulating mouse clicks or inventory actions. Blueprints use relative coordinates, build records are saved by the controller, and the Minecraft execution side can verify placed blocks afterward.

Primary use cases are local Minecraft Java worlds, LAN worlds, creative-mode build iteration, world-aware assistant actions, Web or voice control, and chat-tool entrances for Minecraft operations.

GitHub:

https://github.com/mari0w/blockwright

The next engineering focus is safer existing-world edits, less ambiguous build selection, and more Minecraft operations represented as first-class tools.
