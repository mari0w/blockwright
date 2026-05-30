# Blockwright: Run Minecraft Actions with One Sentence

Minecraft is at its best when you are building, testing ideas, and shaping a world with friends. The repetitive parts are less exciting:

- looking up item IDs just to give yourself a tool;
- switching time or weather while you are building;
- starting a cabin, room, or treehouse from an empty patch of land;
- replacing a wall or changing the glass color in an existing build;
- letting friends in a LAN world send small requests without learning commands.

Blockwright turns those moments into natural-language instructions.

You can type things like:

```text
/bw give me a diamond sword
/bw build a small cabin with windows and a bed
/bw set the time to day
/bw replace the wall in front of me with glass
```

Blockwright sends the player's intent to an AI assistant, then uses the Fabric mod to make the change inside the Minecraft world.

## What is Blockwright?

Blockwright is an AI assistant for Minecraft Java Edition players.

The main installation path is a Fabric mod, designed for Java Edition 1.21.x, single-player saves, and LAN-opened worlds. You do not need to move your map to a Paper server or change how you normally play Minecraft.

After installing it, you can talk to Blockwright through:

- the in-game `/bw ...` command;
- a Web chat page;
- voice input on the Web page;
- Element/Matrix rooms;
- DingTalk bot messages;
- local commands or scripts for custom integrations.

It is not meant to be a chatbot that only replies with text. The goal is to turn chat into real Minecraft actions.

## What can it do?

### 1. Give Items

Instead of remembering full `/give` commands, you can ask:

```text
/bw give me a stack of torches
/bw give me a full diamond kit
/bw give me a sword with knockback
```

This is useful when you want to stay in the flow of building or testing without stopping to search for item IDs.

### 2. Adjust Game State

You can also ask for common world or player changes:

```text
/bw set the time to day
/bw stop the rain
/bw give me night vision
/bw switch me to creative mode
```

These are the small actions players often repeat while building, testing, or playing with friends.

### 3. Build Small Structures

Blockwright can generate and place simple structures from plain language:

```text
/bw build a small cabin
/bw build a room with windows and a bed in front of me
/bw build a simple treehouse with a reachable entrance
```

It is best suited for cabins, rooms, treehouses, platforms, simple decorations, and local structure drafts. You can still edit everything yourself afterward, but you no longer have to start from a blank space.

### 4. Modify What You Are Looking At

Blockwright can also handle local edit requests:

```text
/bw replace this house's windows with blue glass
/bw replace the wall in front of me with stone bricks
/bw make the roof a little higher
```

This makes the interaction feel closer to pointing at something in the world and telling an assistant what to change.

### 5. Continue the Conversation

You do not have to write a perfect request on the first try. You can continue step by step:

```text
/bw build a small cabin
/bw make it bigger
/bw add some lights
/bw change the roof to dark wood
```

That is closer to how players actually build: try a first version, react to it, then refine it.

## How do you start?

The player-facing flow is intentionally simple:

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

Current model options include:

- Codex CLI
- OpenAI
- DeepSeek
- Doubao
- Gemini

After that, you can send requests from the Web page, voice input, the in-game `/bw` command, or connected chat tools.

## Why is this interesting?

Minecraft is about creative freedom, but many repeated actions are just setup work.

Giving items, changing time, clearing rain, drafting a basic room, replacing a wall, or adding light to a build are common tasks. Blockwright tries to make those tasks feel like one sentence, so players can spend more attention on the idea they are building.

It is useful for:

- quickly trying ideas in a single-player save;
- letting friends send requests in a LAN world;
- drafting the first version of a build in creative mode;
- connecting Minecraft to Web, voice, or chat tools;
- experimenting with AI-assisted Minecraft gameplay.

## One-Sentence Summary

Blockwright is not trying to play Minecraft for you.

It is a Minecraft assistant that understands natural language: you describe what you want, and it turns part of the repetitive, command-heavy work into real actions in the world.

If you want Minecraft item giving, building, local edits, and testing to start with one sentence, Blockwright is built for that direction.

## Short Version for X/Twitter

I am building Blockwright, an AI assistant for Minecraft Java Edition.

After installing the Fabric mod, players can type:

```text
/bw give me a diamond sword
/bw build a small cabin
/bw set the time to day
/bw replace this wall with glass
```

It supports Web text, voice, in-game `/bw`, Element/Matrix, DingTalk, and model options including Codex CLI, OpenAI, DeepSeek, Doubao, and Gemini.

The goal is not to play Minecraft for you. It is to turn repeated actions like giving items, changing weather, drafting simple builds, and editing local structures into natural language.
