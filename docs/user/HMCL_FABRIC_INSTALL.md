# HMCL / Fabric 安装方式

这个方式适合你现在的玩法：

```text
HMCL 启动 Minecraft 1.21.8
打开已有单人存档
开放到局域网
别人加入你的局域网世界
```

Blockwright 在这个模式下是 Fabric 模组，不是 Paper 插件，也不需要迁移地图。

## 需要准备

- HMCL
- Minecraft 1.21.8
- Fabric Loader 1.21.8
- Fabric API 1.21.8
- 本项目的 controller
- 本项目构建出来的 `blockwright-fabric-0.1.0.jar`

## 构建模组

在项目目录执行：

```bash
./scripts/build-hmcl-mod.sh
```

构建完成后，模组文件在：

```text
plugins/fabric/build/libs/blockwright-fabric-0.1.0.jar
```

## 放到哪里

把这个 jar 放到 HMCL 当前游戏目录的 `mods` 文件夹。

常见位置类似：

```text
.minecraft/mods/blockwright-fabric-0.1.0.jar
```

如果你在 HMCL 里给 1.21.8 单独设置了游戏目录，就放到那个目录下的：

```text
mods/
```

同时确认 `mods/` 里已经有 Fabric API。

也可以用脚本直接安装：

```bash
./scripts/install-hmcl-mod.sh <HMCL当前游戏目录>
```

这个脚本每次执行都会重新编译 Fabric 模组，并覆盖安装到目标 `mods/` 目录。目标目录里之前已经有 Blockwright jar 时，也会先删除旧的 `blockwright-fabric-*.jar`，再放入本次新编译出来的 jar。

如果你使用默认 `.minecraft`，在项目根目录直接执行下面这个命令即可，它等价于 `./scripts/install-hmcl-mod.sh ~/.minecraft`：

```bash
make
```

如果 HMCL 当前游戏目录不是默认位置：

```bash
make HMCL_DIR=<HMCL当前游戏目录>
```

例如：

```bash
./scripts/install-hmcl-mod.sh ~/.minecraft
```

## 启动 controller

在项目目录执行：

```bash
cargo run -p blockwright-controller
```

默认地址是：

```text
http://127.0.0.1:8765
```

## 启动游戏

1. HMCL 选择 Fabric 1.21.8。
2. 进入你原来的存档。
3. 正常开放到局域网。
4. 在游戏里输入：

```text
/bw 给我一把钻石剑
/bw 帮我盖一个木屋
/bw reload
```

## 建筑怎么执行

建造不是模拟玩家翻背包、切物品、右键慢慢摆方块。

Blockwright 的建筑动作走的是世界方块 API：controller 把蓝图方块列表发给 Fabric 模组，Fabric 模组直接在当前世界里按坐标放置方块。这样不会依赖你的背包里有没有材料，也不会因为物品栏位置不同而失败。

发物品才会进入玩家背包，例如：

```text
/bw 给我一把钻石剑
```

建筑则直接放到世界里，例如：

```text
/bw 帮我盖一个木屋
```

## 建筑一致性

建筑不是只在游戏里“试着盖一下”。controller 会先把这次建筑任务保存到：

```text
data/builds/
```

保存内容包括任务 ID、目标玩家、蓝图 ID、原点、材料统计和完整方块清单。Fabric 模组拿到的也是同一份方块清单。

放置完成后，Fabric 模组会逐块读取世界里的实际方块，并把校验报告回传给 controller。只有实际世界里的方块和构建记录里的方块一致，任务才会标记为成功；如果因为已有建筑保护、单次上限、材质错误或其他原因导致世界里不是预期方块，记录会标记为失败，并保留最多 20 个差异坐标。

## 本地配置

第一次启动模组后，会生成：

```text
.minecraft/config/blockwright.json
```

默认内容等价于：

```json
{
  "controllerUrl": "http://127.0.0.1:8765",
  "serverId": "hmcl-lan",
  "sharedToken": "local-dev-token",
  "connectTimeoutSeconds": 5,
  "requestTimeoutSeconds": 180,
  "protectExistingBlocks": true,
  "maxBlocksPerAction": 5000,
  "scanRadius": 8,
  "scanForwardBlocks": 5,
  "maxScanBlocks": 8000,
  "pollControllerJobs": true,
  "pollIntervalTicks": 40
}
```

正常本机使用不用改。只有 controller 地址或 token 改了才需要改。

`requestTimeoutSeconds` 默认 180 秒，因为启用 Codex CLI 或本地模型后，第一次理解请求可能明显超过 20 秒。游戏里如果提示请求超时，优先确认这个值是否还是旧配置里的 20 秒，必要时改成 180 后执行 `/bw reload` 或重启游戏。

`protectExistingBlocks` 默认是 `true`，意思是蓝图只会放到空气里，遇到已有方块会跳过，避免误覆盖你的旧地图。确认要覆盖已有方块时才改成 `false`。

`maxBlocksPerAction` 是单次动作最多放置多少方块，默认 5000，用来防止误生成超大蓝图卡住存档。

`scanRadius` 默认 8，`scanForwardBlocks` 默认 5，`maxScanBlocks` 默认 8000。它们用于“改造面前这个建筑”这类需求：模组会扫描玩家视线前方附近的非空气方块，把结果发给 controller 匹配已保存的构建记录。

`pollControllerJobs` 默认是 `true`，意思是 Fabric 模组会主动轮询 controller 里的任务队列。钉钉、通用机器人这类本地聊天入口发来的任务，会通过这个轮询进入你的当前世界，不需要公网 webhook。

`pollIntervalTicks` 默认 40 tick，大约 2 秒。

## 版本说明

当前 Fabric 模组目标版本是 Minecraft 1.21.8。其他 1.21.x 版本可能能跑，但不要默认当成已经验证；需要单独构建和测试。

Paper 插件仍然保留在 `plugins/paper`，但那是给独立 Paper 服务器用的，不是你这个 HMCL 局域网存档的主安装方式。

## 图片和复杂建筑

当前本地配置默认启用 Codex CLI。controller 会优先调用本机 `codex exec` 理解自然语言；只要 Codex 是启用状态，Codex 失败时会明确提示失败，不会再退回关键词规则冒充理解。

默认配置使用 `command: "codex --ignore-user-config -m gpt-5.5 -c model_reasoning_effort=low"`。这里的参数会放到 `codex exec` 后面执行，并且 controller 会自动使用 `--json` 读取 session id、用 `--output-schema` 约束模型最终 JSON、用 `--output-last-message` 读取模型最终回复，避免把 Codex CLI 的启动日志、插件日志或 MCP 报错当成模型结果。默认低思考强度，优先保证游戏内响应速度；修改 `config/servers/local.yaml` 后，需要重启 controller。

Codex 会话按人隔离：Minecraft 里同一个玩家连续说话会复用同一个 Codex 会话；不同玩家各自独立。机器人入口按发送人隔离，例如同一个钉钉发送人复用自己的会话。会话映射保存在 `data/codex_sessions.json`，`data/` 已经忽略，不会提交到仓库。

controller 日志不会打印模型原始思考链路，但会打印可排查的阶段进度：`starting codex cli request`、每 10 秒一次的 `codex cli request still running`、`codex blueprint json parsed`、`codex blueprint placement assessed`、`finished codex cli request`。如果 120 秒超时，就能从这些日志判断是 Codex 一直没返回，还是已经返回后卡在解析、场地校验或保存。

建筑类需求默认先走 Codex 蓝图规划，不会先套本地关键词模板。比如“生成一个树屋”“建一个房间”“盖一个木屋”都会先生成并保存新的蓝图，再由模组放置和校验；只有你主动把 `codex.enabled` 改成 `false`，才会使用本地内置蓝图兜底。

游戏内 `/bw ...` 发起新建筑时，Fabric 模组会把玩家面前的附近方块扫描给 controller。controller 会先估算地面高度和落点，再在扫描半径内选择冲突最少、离玩家目标最近的位置。草、花、雪这类软阻挡会自动清理；如果所有合适位置仍然和木头、石头、箱子、已有建筑等硬方块冲突，controller 会自动清理目标体积后继续建造，不会把建筑需求直接拒绝掉。

除了物品和建筑，普通游戏操作也会由 Codex 理解成受控 Minecraft 命令。例如“我想白天”“别下雨”“我想创造模式”“给我夜视”。Fabric 端会做白名单校验，只执行安全范围内的 Minecraft 命令，不执行 `op`、`stop`、`execute`、`fill`、`setblock` 等高风险命令。

启用 Codex 后，图片/复杂文字需求会在 controller 里规划成蓝图 JSON：先保存蓝图，再下发同一份方块清单给 Fabric 模组，最后走上面的逐块校验。也就是说，即使后续接入大模型，游戏里实际盖的内容也必须和保存的蓝图/构建记录一致。

## 改造已有建筑

类似下面这种需求会走“扫描 -> 匹配 -> 改造 -> 校验”的流程：

```text
/bw 把我面前这个房子的窗户换成蓝色玻璃
```

流程是：

1. Fabric 模组扫描玩家视线前方附近的非空气方块。
2. controller 用扫描结果匹配 `data/builds/` 里已经成功完成的构建记录。
3. 如果只匹配到一个建筑，并且能定位到窗户/玻璃方块，就生成改造动作。
4. 改造动作同样会保存成新的构建记录，并在执行后逐块校验。
5. 如果匹配不到、匹配到多个、或者你说的“二楼/正面/左边窗户”定位不清楚，系统会先追问，不会直接乱改。
