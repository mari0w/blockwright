# Blockwright: an open-source AI assistant that acts inside Minecraft Java

I have been building an open-source project called **Blockwright**.

It is an AI assistant for **Minecraft Java Edition**, but the goal is not to make another chatbot that only talks about the game. The goal is to let players describe what they want in natural language, then have the assistant turn that request into real actions inside the Minecraft world.

The first version is focused on the local Fabric experience. After installing the mod, a player can use the in-game `/bw` command, a local Web chat page, voice input from the Web UI, or connected chat tools.

For example:

```text
/bw give me a diamond sword
/bw scan what I am looking at
/bw set the time to day and stop the rain
/bw build a wooden cabin with windows, a bed, and lights
/bw replace the wall in front of me with stone bricks
```

These are examples, but they point to the bigger idea: Minecraft already gives players a powerful creative world, while a lot of useful actions are still command-heavy, repetitive, or slow to prototype.

Reading world context, giving items, changing time, clearing rain, building the first version of a room, replacing a wall, adding lights, or modifying part of a structure are not always the most interesting part of playing. They are often the setup work before the interesting part starts.

Blockwright tries to make that setup work conversational.

What makes the project interesting to me is the execution loop behind it.

The assistant does not just return a paragraph of advice. The controller gives it a controlled tool layer for reading game context, saving plans, enqueueing actions, and sending those actions to the Minecraft side, where the Fabric mod executes them through the server/world API.

For building, that means Blockwright is not trying to fake a player moving a mouse, opening inventory slots, and right-clicking blocks. It places blocks through the world API. Blueprints use relative coordinates. Build records are saved by the controller. The Minecraft execution side can read back world state and report what was actually placed.

The direction is:

```text
natural language request
-> structured plan
-> Minecraft execution
-> verification/reporting
```

This matters because a Minecraft AI assistant should eventually be able to reason about the world, not just generate text. If it builds something, it should know what it tried to build. If it edits something, it should know what structure it is editing. If placement fails, the result should be visible in a report instead of being hidden behind a friendly message.

The current feature set includes:

- Minecraft Java Edition 1.21.x support
- Fabric as the main install path
- In-game `/bw` commands
- A local Web chat UI
- Voice input from the Web UI
- Multiple model backends, including Codex CLI, OpenAI, DeepSeek, Doubao, and Gemini
- Player, inventory, held-item, and nearby-world context tools
- Controlled game actions, including items, time, weather, effects, mode changes, and supported commands
- Building and structure-editing workflows
- Saved blueprints and build records
- Server-side block placement and verification

Right now, the best use cases are local worlds, LAN worlds, creative-mode testing, world-aware assistant actions, build iteration, and experimenting with AI-assisted Minecraft workflows.

I am intentionally starting with the local Fabric path because that is where Minecraft Java players already are. I do not want players to move a single-player world to a separate server just to try an AI assistant. The mod should fit into the way people already play.

There is still a lot to improve.

Existing-world editing needs to become safer and less ambiguous. Build selection needs better UX. The assistant should ask follow-up questions when the target is unclear. More actions should become structured tools instead of prompt-only behavior. The planner needs to keep getting better at Minecraft-specific details like doors, beds, lighting, paths, and block states.

But the core loop is already there, and that is the part I wanted to open up early.

Blockwright is MIT licensed and open source.

GitHub:

https://github.com/mari0w/blockwright

If you are interested in Minecraft mods, local AI agents, game automation, or open-source tooling, I would love feedback on the project.

The question I keep coming back to is:

What should an AI assistant inside Minecraft be able to do, once it can actually act in the world instead of only talking about it?
