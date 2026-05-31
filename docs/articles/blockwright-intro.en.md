# Blockwright: Production AI Control for Minecraft Java Edition

Minecraft is at its best when you are building, testing ideas, and shaping a world with friends. The friction usually starts when you have to stop playing and translate an idea into commands:

- checking what you are holding or what block you are looking at;
- looking up item IDs, effect names, time and weather commands;
- asking for a structure and then refining it step by step;
- changing part of an existing build without manually replacing every block;
- letting friends in a LAN world send real requests without learning command syntax.

Blockwright turns those moments into natural-language instructions that a Minecraft AI assistant can carry out through controlled tools, command operations, build records, and verification reports.

You can type things like:

```text
/bw scan what I am looking at and tell me what block it is
/bw set the time to day and stop the rain
/bw build a cabin with windows, a bed, lights, and a reachable door
/bw replace the wall in front of me with stone bricks
```

Blockwright sends the player's intent to an AI assistant, then uses the Fabric mod to read the world, execute actions, place or edit blocks, and report back what happened.

## What is Blockwright?

Blockwright is a production-oriented AI control layer for Minecraft Java Edition.

The main installation path is a Fabric mod, designed for Java Edition 1.21.x, single-player saves, and LAN-opened worlds. You do not need to move your map to a Paper server or change how you normally play Minecraft.

After installing it, you can talk to Blockwright through:

- the in-game `/bw ...` command;
- a Web chat page;
- voice input on the Web page;
- Element/Matrix rooms;
- DingTalk bot messages;
- local commands or scripts for custom integrations.

It is not limited to chat replies, item delivery, or building prompts. Blockwright gives a Minecraft AI assistant a controlled tool layer for reading world context, operating supported commands, building, editing, and verifying real results inside the world.

## What can it do?

### 1. Read Player and World Context

Blockwright can work from real game context instead of only guessing from chat text:

```text
/bw what am I holding
/bw check my inventory
/bw scan the blocks near me
```

That context is useful before edits, build changes, item requests, and ordinary gameplay commands.

### 2. Execute Controlled Game Actions

The assistant can run supported actions through the controller and Minecraft execution side:

```text
/bw give me a stack of torches
/bw set the time to day
/bw stop the rain
/bw give me night vision
/bw switch me to creative mode
```

This keeps common operations conversational while still routing them through explicit action types and server-side APIs.

### 3. Build With Records and Verification

For build requests, Blockwright saves the plan before Minecraft executes it:

```text
/bw build a room with windows and a bed in front of me
/bw build a simple treehouse with a reachable entrance
/bw add a lit path from here to the door
```

Blueprints and build records are kept by the controller, then the Fabric mod places blocks through server-side world APIs and reads the world back for verification.

### 4. Modify What You Are Looking At

Blockwright can also handle local edit requests:

```text
/bw replace this house's windows with blue glass
/bw replace the wall in front of me with stone bricks
/bw make the roof a little higher
```

This makes the interaction feel closer to pointing at something in the world and telling an assistant what to change.

### 5. Continue the Conversation

You do not have to compress the entire operation into one message. You can continue step by step:

```text
/bw build a cabin
/bw make it bigger
/bw add some lights
/bw change the roof to dark wood
```

That matches how players actually build: place a structure, inspect it in the world, then refine it through follow-up requests.

## How do you start?

The player-facing flow:

1. Prepare Minecraft Java Edition 1.21.x.
2. Install Fabric Loader 0.16.14 or newer.
3. Put these files in your current game directory's `mods/` folder:
   - Fabric API for your Minecraft 1.21.x version
   - `blockwright-fabric-*.jar`
4. Start Minecraft and enter a world.
5. Run this command in Minecraft chat:

```text
/bw web
```

Open the displayed Web address, set your current Minecraft player name, then choose an AI model.

Supported model backends:

- Codex CLI
- OpenAI
- DeepSeek
- Doubao
- Gemini

After that, you can send requests from the Web page, voice input, the in-game `/bw` command, or connected chat tools.

## Interface Screenshots

These screenshots are ready to use on the website, tutorials, X/Twitter, forums, or other publishing platforms.

![Blockwright mobile chat window](../assets/web-chat-mobile-preview.png?v=20260530)

The mobile chat screenshot shows that players can use text or voice from a phone browser.

![Blockwright Web settings page](../assets/web-settings-preview.png?v=20260530)

The Web settings screenshot shows the player name, language, Controller Token, and AI model configuration entry.

![Blockwright supported AI model dropdown](../assets/web-model-provider-dropdown.png?v=20260530)

The AI model dropdown shows Codex CLI, OpenAI, DeepSeek, Doubao, and Gemini.

## Why this matters

Minecraft is about creative freedom, but many repeated actions are just setup work or command syntax.

Reading local world state, giving items, changing time, clearing rain, drafting a room, replacing a wall, or adding light to a build are common tasks. Blockwright turns those tasks into one controlled assistant workflow, so players and operators can spend more attention on the world they are shaping while still keeping execution explicit and traceable.

It is useful for:

- operating a single-player save with AI-assisted world context;
- letting friends send requests in a LAN world;
- creating and refining builds in creative mode;
- connecting Minecraft to Web, voice, or chat tools;
- running Minecraft actions through a controlled AI assistant workflow.

## One-Sentence Summary

Blockwright is a Minecraft AI assistant that understands natural language and can use a controlled tool layer: you describe what you want, and it turns repetitive, command-heavy work into real actions in the world.

It is designed for AI-operated Minecraft workflows where context reads, supported commands, building, editing, records, and verification all matter.

## Short Version for X/Twitter

Blockwright is a production-oriented AI control layer for Minecraft Java Edition.

After installing the Fabric mod, players can type:

```text
/bw give me a diamond sword
/bw scan what I am looking at
/bw build a cabin with lights and a bed
/bw replace this wall with stone bricks
```

It supports Web text, voice, in-game `/bw`, Element/Matrix, DingTalk, and model options including Codex CLI, OpenAI, DeepSeek, Doubao, and Gemini.

It lets a Minecraft AI assistant operate through controlled tools: read context, run supported actions, build, edit, record, and verify what changed.
