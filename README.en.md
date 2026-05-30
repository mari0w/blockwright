# Blockwright

<p align="center">
  <img src="docs/assets/blockwright-logo.png" alt="Blockwright logo" width="160" height="160">
</p>

<p align="center">
  <a href="https://github.com/mari0w/blockwright/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/mari0w/blockwright/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/mari0w/blockwright/actions/workflows/universal-fabric-mod.yml"><img alt="Universal Fabric Mod" src="https://github.com/mari0w/blockwright/actions/workflows/universal-fabric-mod.yml/badge.svg"></a>
  <a href="https://github.com/mari0w/blockwright/actions/workflows/ci.yml"><img alt="Coverage gate 80%+" src="https://img.shields.io/badge/coverage%20gate-80%25%2B-brightgreen.svg"></a>
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-green.svg"></a>
  <img alt="Minecraft 1.21.x" src="https://img.shields.io/badge/Minecraft-1.21.x-62B47A.svg">
  <img alt="Fabric Loader 0.16.14+" src="https://img.shields.io/badge/Fabric%20Loader-0.16.14%2B-DBD0B4.svg">
  <img alt="Java 21+" src="https://img.shields.io/badge/Java-21%2B-orange.svg">
  <a href="https://github.com/mari0w/blockwright/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/mari0w/blockwright?style=social"></a>
</p>

English | [简体中文](README.zh-CN.md)

Blockwright is an AI assistant for Minecraft Java Edition players. After installing the Fabric mod, start the game and ask it in natural language to give items, change time or weather, build houses, edit existing structures, or run ordinary game actions.

You can type on the Web page, hold the microphone button to talk, use the in-game `/bw` command, or connect chat tools such as Element/Matrix and DingTalk.

## What It Does

- Give items: `give me a diamond sword`, `give me torches`, `give me full diamond armor`.
- Change game state: `make it daytime`, `stop the rain`, `switch me to creative mode`.
- Build: `build a small wooden cabin`, `make a room in front of me with windows and a bed`.
- Edit builds: `replace this house's windows with blue glass`, `change this wall to stone bricks`.
- Continue a conversation for the same player: `make it bigger`, `make the roof higher`, `continue that build`.

## How to Use

### 1. Prepare the Game and Mods

Prepare Minecraft Java Edition `1.21.x` and install Fabric Loader `0.16.14` or newer.

Put these mods into the current game directory's `mods/` folder:

- Fabric API for your 1.21.x game version
- Blockwright Fabric mod: `blockwright-fabric-*.jar`

If your launcher uses a separate game directory for this 1.21.x profile, use that profile's `mods/` folder.

### 2. Start Minecraft

Start Minecraft with the Fabric profile and enter your world. Blockwright is ready with the game, so players do not need to start any extra program.

### 3. Open the Web Page

In Minecraft chat, run:

```text
/bw web
```

Open the Web address it prints. On the same computer, it is usually:

```text
http://127.0.0.1:8765/web
```

### 4. Set Your Minecraft Username

The first time you open the Web page, enter your Minecraft username. Use the exact name shown in game, because Web text and voice commands are sent to that player.

You can change it later from **Player > Minecraft username** in the Web settings.

### 5. Choose an AI Model

Open the Web settings and choose **AI model**. Blockwright currently supports:

- Codex CLI
- OpenAI
- DeepSeek
- Doubao
- Gemini

After choosing a model and completing the settings shown on the page, you can start sending requests.

### 6. Send Requests

You can use three common entry points:

- Type in the Web page.
- Click the microphone button on the Web page, hold to talk, and release to send.
- Type `/bw ...` in Minecraft chat.

Examples:

```text
/bw give me a diamond sword
/bw build me a wooden cabin with windows and a bed
/bw make it daytime
/bw replace this wall with glass
```

## Supported Entry Points and Chat Tools

- **Web text chat**: type directly in the browser.
- **Web voice input**: use a phone or computer microphone; phones usually need the HTTPS address and microphone permission.
- **Minecraft command**: type `/bw ...` directly in game.
- **Element/Matrix**: send room messages to the current Minecraft player.
- **DingTalk bot**: supports DingTalk Stream mode.
- **Local command/custom script entry**: useful for connecting other local chat tools or automation scripts.

## In-Game Commands

| Command | Purpose |
| --- | --- |
| `/bw <request>` | Send a natural-language request, such as giving items, building, editing, or changing weather. |
| `/bw ask <request>` | Explicitly send an AI chat/planning request. |
| `/bw chat <request>` | Same idea as `/bw ask`; sends a chat/planning request. |
| `/bw web` | Print the Web page address in Minecraft chat. |
| `/bw config` | Point you to Web settings for player name, model, and chat tools. |
| `/bw url` / `/bw address` / `/bw lan` | Show the Web address, similar to `/bw web`. |

## Who It Is For

- Players who want to control Minecraft with one sentence.
- Server owners or operators who want to connect Minecraft to Web, voice, or chat tools.
- Anyone who wants to try AI-assisted Minecraft building and gameplay.

## License

Blockwright is licensed under the [MIT License](LICENSE).
