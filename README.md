# Blockwright

`Blockwright` 是一个面向 Minecraft 的本地智能建造/物品助手项目。

目标不是把 AI 逻辑塞进 Minecraft 插件里，而是拆成两层：

- `apps/controller`：Rust/Axum 本地控制器，负责聊天机器人入口、Codex CLI 适配、蓝图数据库、构建记录、任务队列和规划逻辑。
- `plugins/fabric`：HMCL/单人存档/局域网开放世界使用的 Fabric 模组，负责游戏内命令、发物品、放方块、轮询外部机器人下发的任务。
- `plugins/paper`：独立 Paper 服务端插件，保留给真正跑 Paper 服务器的场景。

这样做的好处是后面要接 Telegram、Discord、企业微信、图片分析、数据库、Codex 命令行，都在 controller 里扩展；Minecraft 插件只保留稳定的游戏内执行能力。

## 当前已经具备的能力

- 游戏内执行 `/bw ask <需求>`，把需求发给本地 controller。
- 外部机器人可以调用 `POST /api/robot/message`，controller 会把任务放进 Minecraft 任务队列。
- Fabric/Paper 执行端定时轮询 `GET /api/minecraft/jobs/next`，拿到任务后在当前世界里执行。
- 支持基础动作：
  - `give_item`：给玩家物品。
  - `place_blocks`：按蓝图放置方块。
  - `chat`：返回说明消息。
- 蓝图以 JSON 文件保存，能表达材料清单、尺寸、相对坐标和标签。
- 建筑任务会保存构建记录，执行端放置后逐块校验世界状态，并把校验报告回写 controller。
- 对“改造面前建筑”这类需求，Fabric 会扫描附近非空气方块，controller 会先匹配已保存构建记录；匹配不唯一或部位不明确时只追问，不直接下发改造。
- 默认接入 Codex CLI 做自然语言理解；controller 会把动作 JSON、蓝图 JSON、相对坐标和一致性规则写进规划 prompt，模型产出的蓝图会先保存再下发执行。

## 快速启动 controller

```bash
cp .env.example .env
cargo run -p blockwright-controller
```

默认监听：

```text
http://127.0.0.1:8765
```

默认日志级别是 `info`，不会打印 Fabric 每 2 秒一次的任务轮询请求。需要临时排查 HTTP 请求时再这样启动：

```bash
RUST_LOG=info,tower_http=debug cargo run -p blockwright-controller
```

健康检查：

```bash
curl http://127.0.0.1:8765/health
```

## 测试和覆盖率

controller 单元测试和 API 集成测试：

```bash
cargo test --workspace
```

controller 覆盖率门禁要求不低于 80%：

```bash
cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80
```

Paper 插件测试：

```bash
cd plugins/paper
gradle test
```

全量本地检查也可以直接跑：

```bash
make test
```

## 聊天工具接入

controller 支持把不同聊天入口统一成一类消息，再转成 Minecraft 任务：

- Minecraft 游戏内命令：`/bw <需求>`、`/bw ask <需求>`、`/bw chat <需求>`。
- 通用本地机器人入口：`POST /api/robot/message`，支持文字和图片附件。
- 钉钉应用机器人：使用 Stream 模式接收消息，适合本地运行，不需要公网 webhook 回调地址。

真实聊天工具配置默认放在：

```text
config/chat.local.yaml
```

这个文件已加入 `.gitignore`，不要提交真实 webhook、client secret 或 token。仓库只保留示例：

```bash
cp config/chat.example.yaml config/chat.local.yaml
```

钉钉接入要使用“应用机器人 + Stream 模式”。不要用“群自定义机器人 Webhook”作为接收入口，因为它只能发群消息，不能接收用户消息，也不适合本地 Minecraft 场景。

模拟游戏内命令：

```bash
curl -X POST http://127.0.0.1:8765/api/minecraft/message \
  -H 'Content-Type: application/json' \
  -d '{"server_id":"local-paper","player":"Steve","text":"给我一把钻石剑","position":{"world":"world","x":0,"y":64,"z":0}}'
```

如果返回里带有 `job_id`，说明这次包含建筑动作。执行端会在放置后回传校验结果；也可以用接口查看构建记录：

```bash
curl http://127.0.0.1:8765/api/builds/hm-job-1
```

模拟外部机器人下发建造任务：

```bash
curl -X POST http://127.0.0.1:8765/api/robot/message \
  -H 'Content-Type: application/json' \
  -d '{"platform":"telegram","conversation_id":"local","sender":"charles","server_id":"local-paper","target_player":"Steve","text":"帮我盖一个木屋"}'
```

## HMCL / Fabric 模组

如果你是用 HMCL 打开现有单人存档，然后“开放到局域网”给别人加入，应该使用 Fabric 模组，不需要新建 Paper 服务端，也不需要迁移地图。

局域网玩法下，只需要房主这台电脑安装 Blockwright 模组并运行 controller。其他玩家加入你的局域网世界后，可以直接在聊天栏使用 `/bw ...` 调用你这台电脑上的本地 controller 和模型，不需要每个人电脑都装 Blockwright。前提是当前只使用原版方块/物品和服务端命令能力；如果以后加入自定义方块、客户端界面或专属资源包，才需要让其他玩家同步安装对应客户端内容。

### 房主部署步骤

1. 在 HMCL 里选择或安装 Minecraft `1.21.8` 的 Fabric Loader。
2. 把 Fabric API 放进当前游戏目录的 `mods/`。
3. 执行安装脚本。这个脚本每次都会重新编译 Blockwright 模组，并覆盖安装到当前游戏目录的 `mods/`：

```bash
./scripts/install-hmcl-mod.sh <HMCL当前游戏目录>
```

例如默认 `.minecraft`：

```bash
./scripts/install-hmcl-mod.sh ~/.minecraft
```

4. 启动本地 controller：

```bash
cargo run -p blockwright-controller
```

5. 用 HMCL 进入你原来的单人存档，正常“开放到局域网”。

6. 你或加入局域网的玩家在聊天栏输入：

```text
/bw 给我一把钻石剑
/bw 帮我盖一个木屋
/bw 把我面前这个房子的窗户换成蓝色玻璃
```

controller 地址默认是 `http://127.0.0.1:8765`。因为 Fabric 模组运行在房主电脑上，所以这个地址不用改成局域网 IP；其他玩家也不需要访问自己的 `127.0.0.1`。

第一次接入 Codex CLI 或本地模型时，规划可能超过 20 秒。Fabric 配置里的 `requestTimeoutSeconds` 默认是 180；如果你的 `.minecraft/config/blockwright.json` 是旧版本生成的，手动补上或改成：

```json
{
  "requestTimeoutSeconds": 180
}
```

改完后执行 `/bw reload` 或重启游戏。

Fabric 模组源码在：

```text
plugins/fabric
```

构建脚本生成的 jar 路径：

```text
plugins/fabric/build/libs/blockwright-fabric-0.1.0.jar
```

详细步骤见：

```text
docs/user/HMCL_FABRIC_INSTALL.md
```

## Paper 插件

如果你单独运行 Paper 服务端，才使用这个方式。HMCL 单人存档/局域网开放世界不需要走 Paper。

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

建筑执行记录默认放在：

```text
data/builds/
```

这里保存的是每次实际下发的方块清单和执行端校验报告。规则是：controller 先保存构建记录，再下发同一份 `place_blocks`；Fabric/Paper 放置后会读取世界里的实际方块，如果 `verified_count` 不等于计划数量，或者出现 `mismatches`，该构建记录会标记为失败。

改造已有建筑时不会只按“玩家面前大概有个房子”来猜。HMCL/Fabric 模组会在改造类指令里自动附带附近扫描结果，controller 再用这个扫描结果匹配 `data/builds/` 中状态为 `succeeded` 的构建记录。只有匹配唯一、目标部位能定位时，才会生成新的改造构建记录并执行。

## AI 规划边界

本地配置 `config/servers/local.yaml` 默认 `codex.enabled: true`，controller 会优先调用本机 `codex exec` 理解玩家自然语言。比如“钻石稿子/钻石镐子/diamond pickaxe”应由 Codex CLI 理解成 `minecraft:diamond_pickaxe`，不会只因为包含“钻石”就发 64 个钻石。

默认命令是：

```yaml
codex:
  enabled: true
  command: "codex --ignore-user-config -m gpt-5.5"
  timeout_seconds: 120
```

这里的 `command` 只写 `codex exec` 的参数，controller 会自动补上 `exec`、`--ephemeral` 和 `--output-last-message`。这样不会继承你全局 `~/.codex/config.toml` 里的 MCP、插件和其他项目配置，游戏里的每次请求也不会污染 Codex 会话历史。改完这个配置后需要重启 controller。

controller 不会把大模型放进 Minecraft 模组里，而是在 `apps/controller` 里调用 Codex CLI。调用时会把这些规则作为 prompt 的硬性约束：

- 只输出蓝图 JSON，不输出命令步骤。
- 方块坐标必须是相对坐标。
- 材质必须是 `minecraft:xxx`。
- 蓝图先保存到 `data/blueprints/`，再生成 `place_blocks`。
- 执行结果必须通过 `data/builds/` 的逐块校验报告确认。

如果本机暂时没有登录或安装 Codex CLI，可以把 `config/servers/local.yaml` 里的 `codex.enabled` 改成 `false`，controller 会退回内置规则兜底。

请求进入 controller 后会先打印 `received minecraft message`；开始调用模型时会打印 `starting codex cli request`，结束时会打印 `finished codex cli request`。如果游戏里显示请求失败但 controller 没有这两类日志，先检查游戏里的 `controllerUrl` 是否指向 `http://127.0.0.1:8765`。

如果你要让 Codex CLI 走本地模型，可以把 `command` 改成带参数的形式，例如：

```yaml
codex:
  enabled: true
  command: "codex --oss"
  timeout_seconds: 120
```

也可以用你自己的 Codex profile：

```yaml
codex:
  enabled: true
  command: "codex --profile local"
  timeout_seconds: 120
```

## 后续扩展方向

1. 接入 Telegram/Discord/企业微信等机器人，优先选择 polling、stream 或 local_command 这类本地友好的入口。
2. 加图片分析流水线：图片 -> 结构识别 -> 材料映射 -> 蓝图 JSON。
3. 扩展更多改造指令：楼层、朝向、局部区域、材质替换和撤销。
4. 数据层从 JSON 文件升级为 SQLite/Postgres，保留同一套蓝图领域模型。
