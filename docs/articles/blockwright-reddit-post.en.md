# I built Blockwright, an open-source AI assistant that can perform actions inside Minecraft Java

Hey everyone, I have been building an open-source project called **Blockwright**.

It is an AI assistant for **Minecraft Java Edition**. The goal is to make an assistant that does more than talk about Minecraft. I want it to understand natural-language requests and turn them into real in-game actions.

After installing the Fabric mod, you can type commands like:

```text
/bw give me a diamond sword
/bw scan what I am looking at
/bw set the time to day and stop the rain
/bw build a wooden cabin with windows, a bed, and lights
/bw replace the wall in front of me with stone bricks
```

Blockwright currently supports Minecraft Java Edition 1.21.x, with Fabric as the main install path. You can use it from the in-game `/bw` command, a local Web chat page, voice input from the Web UI, or connected chat tools. It also supports multiple model backends, including Codex CLI, OpenAI, DeepSeek, Doubao, and Gemini.

The current feature set includes player/world context reads, controlled game actions, supported command actions, build and structure-editing workflows, saved blueprints, and build records. It sends structured actions to the Minecraft side for execution instead of pretending a chat reply changed the world.

The part I care about most is making this more reliable than a chatbot that just writes commands. Block placement is done through the server/world API, not by simulating mouse clicks or inventory actions. Blueprints use relative coordinates, build records are saved by the controller, and the Minecraft execution side can verify placed blocks afterward.

The project is still early, but the direction is:

```text
natural language request -> structured plan -> Minecraft execution -> verification/reporting
```

Right now the best use cases are local Minecraft Java worlds, LAN worlds, creative-mode testing, world-aware assistant actions, build iteration, and experimenting with AI-assisted Minecraft workflows.

GitHub:

https://github.com/mari0w/blockwright

I would love feedback from people who build mods, run servers, or experiment with Minecraft automation.

The main things I am thinking about next are how to make existing-world edits safer, how to make build selection less ambiguous, and which Minecraft operations should become first-class tools next.
