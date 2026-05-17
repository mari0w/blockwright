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

当前本地配置默认启用 Codex CLI。controller 会优先调用本机 `codex exec` 理解自然语言；如果 Codex CLI 没安装、没登录或执行失败，controller 会退回内置规则兜底。

默认配置使用 `command: "codex --ignore-user-config -m gpt-5.5"`。这里的参数会放到 `codex exec` 后面执行，并且 controller 会自动使用 `--output-last-message` 读取模型最终回复，避免把 Codex CLI 的启动日志、插件日志或 MCP 报错当成模型结果。修改 `config/servers/local.yaml` 后，需要重启 controller。

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
