# Blockwright

`Blockwright` 是一个面向 Minecraft 的本地智能建造/物品助手项目。

目标不是把 AI 逻辑塞进 Minecraft 插件里，而是拆成两层：

- `apps/controller`：Rust/Axum 本地控制器，负责聊天机器人入口、Codex CLI 适配、蓝图数据库、任务队列和规划逻辑。
- `plugins/paper`：Minecraft Paper 服务端插件，负责游戏内命令、发物品、放方块、轮询外部机器人下发的任务。

这样做的好处是后面要接 Telegram、Discord、企业微信、图片分析、数据库、Codex 命令行，都在 controller 里扩展；Minecraft 插件只保留稳定的游戏内执行能力。

## 当前已经具备的能力

- 游戏内执行 `/bw ask <需求>`，把需求发给本地 controller。
- 外部机器人可以调用 `POST /api/robot/message`，controller 会把任务放进 Minecraft 任务队列。
- Paper 插件定时轮询 `GET /api/minecraft/jobs/next`，拿到任务后在服务器里执行。
- 支持基础动作：
  - `give_item`：给玩家物品。
  - `place_blocks`：按蓝图放置方块。
  - `chat`：返回说明消息。
- 蓝图以 JSON 文件保存，能表达材料清单、尺寸、相对坐标和标签。
- 预留 Codex CLI 适配层，后续可把自然语言和图片分析交给本地 Codex 执行。

## 快速启动 controller

```bash
cp .env.example .env
cargo run -p blockwright-controller
```

默认监听：

```text
http://127.0.0.1:8765
```

健康检查：

```bash
curl http://127.0.0.1:8765/health
```

模拟游戏内命令：

```bash
curl -X POST http://127.0.0.1:8765/api/minecraft/message \
  -H 'Content-Type: application/json' \
  -d '{"server_id":"local-paper","player":"Steve","text":"给我一把钻石剑","position":{"world":"world","x":0,"y":64,"z":0}}'
```

模拟外部机器人下发建造任务：

```bash
curl -X POST http://127.0.0.1:8765/api/robot/message \
  -H 'Content-Type: application/json' \
  -d '{"platform":"telegram","conversation_id":"local","sender":"charles","server_id":"local-paper","target_player":"Steve","text":"帮我盖一个木屋"}'
```

## Paper 插件

插件源码在 `plugins/paper`。

构建前确认本机有 JDK 21 和 Gradle：

```bash
cd plugins/paper
gradle build
```

把生成的 jar 放到 Paper 服务端 `plugins/` 目录，启动服务端后配置：

```yaml
controller-url: "http://127.0.0.1:8765"
server-id: "local-paper"
shared-token: "local-dev-token"
poll-interval-ticks: 40
```

游戏内命令：

```text
/bw ask 给我一把钻石剑
/bw ask 帮我盖一个木屋
/bw reload
```

## 蓝图模型

蓝图是一个可持久化的建筑图。它记录：

- 建筑 ID、名称、描述。
- 尺寸。
- 材料清单。
- 每个方块相对原点的位置和 Minecraft 材质名。
- 标签，例如 `house`、`starter`、`wood`。

示例见：

```text
blueprints/examples/oak_house.json
```

运行时保存的蓝图默认放在：

```text
data/blueprints/
```

## 后续扩展方向

1. 把 controller 的规则规划器替换/增强为 Codex CLI 调用。
2. 接入 Telegram/Discord/企业微信机器人 webhook。
3. 加图片分析流水线：图片 -> 结构识别 -> 材料映射 -> 蓝图 JSON。
4. 插件增加建筑扫描能力：玩家面前区域 -> 方块矩阵 -> 识别已有蓝图 -> 局部修改。
5. 数据层从 JSON 文件升级为 SQLite/Postgres，保留同一套蓝图领域模型。
