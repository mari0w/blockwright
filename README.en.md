# Blockwright

<p align="center">
  <img src="docs/assets/blockwright-logo.png" alt="Blockwright logo" width="160" height="160">
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

### 1. Install the Mod

Prepare Minecraft Java Edition `1.21.8`, Fabric Loader, and Fabric API. Put the released `blockwright-fabric-*.jar` into the current game directory's `mods` folder.

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
