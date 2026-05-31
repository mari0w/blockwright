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

[English](README.md) | 简体中文

Blockwright 是一个给 Minecraft Java 版玩家用的 AI 助手。装好 Fabric 模组后，打开游戏就可以用自然语言让它发物品、调时间和天气、建造房子、改造已有建筑，或者执行一些普通游戏操作。

你可以通过 Web 页面打字、按住麦克风说话、在游戏内输入 `/bw`，也可以把 Element/Matrix、钉钉等聊天工具接进来。

## 主要能做什么

- 发物品：`给我一把钻石剑`、`给我一组火把`、`给我一套钻石装备`。
- 改游戏状态：`把时间调到白天`、`别下雨了`、`切到创造模式`。
- 建造：`帮我盖一个小木屋`、`在我面前建一个带窗户和床的房间`。
- 改造：`把这个房子的窗户换成蓝色玻璃`、`把我面前这面墙换成石砖`。
- 继续对话：可以围绕同一个玩家继续说“接着改”“换大一点”“把屋顶加高”。

## 使用步骤

### 1. 准备游戏和模组

准备 Minecraft Java Edition `1.21.x` 系列，并安装 Fabric Loader `0.16.14` 或更新版本。

需要放进当前游戏目录 `mods/` 文件夹的模组：

- 对应 1.21.x 游戏版本的 Fabric API
- Blockwright Fabric 模组：`blockwright-fabric-*.jar`

如果你在启动器里给这个 1.21.x 游戏版本设置了单独的游戏目录，就放到那个目录下的 `mods/` 文件夹。

### 2. 启动游戏

用 Fabric 配置启动 Minecraft，进入你的世界。Blockwright 会随游戏一起准备好，不需要玩家另外开任何程序。

### 3. 打开 Web 端

在 Minecraft 聊天栏输入：

```text
/bw web
```

然后打开它显示的 Web 地址。本机通常是：

```text
http://127.0.0.1:8765/web
```

### 4. 填自己的 Minecraft 用户名

第一次打开 Web 端时，页面会让你填写 Minecraft 用户名。这里要填游戏里显示的准确名字，因为 Web 端文字和语音都会发给这个玩家。

以后想修改，可以到 Web 端右上角设置里的 **玩家 > Minecraft 用户名** 修改。

### 5. 配置大模型

在 Web 端右上角设置里选择 **AI 模型**。目前支持：

- Codex CLI
- OpenAI
- DeepSeek
- 豆包 Doubao
- Gemini

选好模型并按页面提示完成配置后，就可以开始发指令。

### 6. 开始使用

你可以用三种常用方式发需求：

- 在 Web 端输入文字。
- 在 Web 端点击麦克风，按住说话，松手发送。
- 在 Minecraft 聊天栏输入 `/bw ...`。

例如：

```text
/bw 给我一把钻石剑
/bw 帮我盖一个带窗户和床的小木屋
/bw 把时间调到白天
/bw 把我面前这面墙换成玻璃
```

## 支持的入口和聊天工具

- **Web 端文字聊天**：适合在浏览器里直接打字。
- **Web 端语音**：适合手机或电脑麦克风输入；手机一般需要使用 HTTPS 地址并允许麦克风权限。
- **Minecraft 命令**：在游戏内直接输入 `/bw ...`。
- **Element/Matrix**：支持通过房间消息把需求发到当前 Minecraft 玩家。
- **钉钉机器人**：支持钉钉 Stream 模式接入。
- **本地命令/自定义脚本入口**：适合把其他本地聊天工具或自动化脚本接进来。

## 游戏内命令

| 命令 | 用途 |
| --- | --- |
| `/bw <需求>` | 发送一条自然语言指令，例如发物品、建造、改造、调天气。 |
| `/bw web` | 在游戏聊天里显示 Web 端访问地址。 |

## 适合谁用

- 想在 Minecraft 里用一句话完成物品、建造和改造操作的玩家。
- 想把 Minecraft 世界接到 Web、语音或聊天工具里的服主/运营。
- 想体验 AI 参与 Minecraft 建造和游戏操作的人。

## 许可证

Blockwright 使用 [MIT License](LICENSE)。
