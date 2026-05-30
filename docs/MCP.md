# Blockwright MCP

Blockwright MCP 是 controller 的可选 stdio 入口，用来让 Codex、机器人或其他 MCP 客户端以“普通 Minecraft 助手”的方式调用 Blockwright。

MCP 是基础工具面：读取玩家状态、读取手持物和物品栏、扫描附近方块、给物品、放方块、执行 Minecraft 命令、保存/删除蓝图、搜索构建记录、入队受控动作。AI 负责聊天、判断、调用工具和使用 skills；controller 只做工具运行时、协议校验、任务队列和执行边界。

它仍然不是裸 Minecraft API MCP。不要通过 MCP 暴露背包模拟操作；Minecraft 命令统一走 `run_command`，建筑改世界仍然走 Fabric/Paper 执行端、构建记录和逐块校验。

## 启动

```bash
cargo run -p blockwright-controller -- mcp
```

普通 HTTP controller 仍然这样启动：

```bash
cargo run -p blockwright-controller
```

## 设计边界

MCP 可以做：

- 解释 Blockwright 动作协议。
- 让 AI 用自然语言请求 Blockwright 规划动作。
- 读取在线玩家的主手、副手、快捷栏和物品栏快照。
- 按半径扫描玩家附近非空气方块。
- 直接给物品、放置明确方块、执行 Minecraft 命令或发送聊天。
- dry-run 返回计划。
- `execute=true` 时把受控动作入队给 Minecraft 执行端。
- 查询、保存和删除蓝图。
- 查询或删除构建记录，并按坐标搜索附近构建。
- 校验蓝图 JSON。
- 当 AI 已经知道准确动作时，直接入队 `give_item`、`place_blocks`、`run_command` 或 `chat`。

MCP 不做：

- 不模拟玩家翻背包、选物品、右键放置。
- 不绕过 controller 的构建记录和执行端校验。

读取类工具不是裸 Minecraft API。MCP 发起的是 Blockwright 高层查询，真正读取仍由 Fabric/Paper 插件在服务端世界里完成，然后把结构化结果返回给 MCP。

## 主要工具

### `blockwright_assistant_message`

高层助手入口。默认只 dry-run，不执行。

常用参数：

- `text`：自然语言需求。
- `server_id`：Minecraft server id，默认使用配置里的 `minecraft.default_server_id`。
- `target_player`：目标玩家。
- `sender`：会话身份，默认 `mcp`。
- `conversation_id`：会话 ID，默认 `local`。
- `execute`：默认 `false`；为 `true` 时才入队执行。
- `position`：可选玩家位置。
- `nearby_scan`：可选世界扫描结果。

示例：

```json
{
  "text": "在我面前盖一个小木屋",
  "server_id": "local-java",
  "target_player": "Steve",
  "execute": false
}
```

执行时：

```json
{
  "text": "给 Steve 一把钻石剑",
  "server_id": "local-java",
  "target_player": "Steve",
  "execute": true
}
```

### `blockwright_protocol`

返回 Blockwright 的安全动作协议和边界，适合 AI 在规划前读取。

### `blockwright_get_player_state`

读取在线玩家当前状态，返回：

- `selected_slot`
- `main_hand`
- `off_hand`
- `inventory[]`

常用参数：

- `server_id`：Minecraft server id，默认使用配置里的 `minecraft.default_server_id`。
- `target_player`：在线玩家名；不填时由执行端选择当前在线玩家。
- `timeout_seconds`：等待插件回包时间，默认 8 秒。

### `blockwright_scan_nearby_blocks`

让 Fabric/Paper 按玩家当前位置和视线前方扫描附近非空气方块，返回 `WorldScan`。

常用参数：

- `server_id`
- `target_player`
- `radius`：扫描半径，默认使用插件配置，最大 32。
- `timeout_seconds`：默认 10 秒。

### 直接操作工具

- `blockwright_give_item`：发放物品，并要求执行端把物品切到玩家主手可见的快捷栏。
- `blockwright_place_blocks`：把明确方块列表放到指定 `origin`，controller 会保存构建记录，执行端会逐块校验。
- `blockwright_run_command`：执行 Minecraft 指令，不做命令白名单限制。
- `blockwright_send_chat`：向 Minecraft 发送聊天消息。

### `blockwright_validate_blueprint`

校验蓝图是否满足基本结构要求，例如：

- 材料是否使用 `minecraft:` 命名空间。
- `materials` 统计是否和 `blocks` 一致。
- 普通蓝图是否错误使用负相对高度。

### 蓝图和构建记录查询

- `blockwright_list_blueprints`
- `blockwright_get_blueprint`
- `blockwright_save_blueprint`
- `blockwright_delete_blueprint`
- `blockwright_list_builds`
- `blockwright_get_build`
- `blockwright_delete_build`
- `blockwright_search_builds_nearby`
- `blockwright_health`

### `blockwright_enqueue_actions`

直接入队受控 Minecraft 动作。适合 AI 已经通过上下文、MCP 读取或 skills 得到了明确动作数据，不需要再走自然语言规划桥。

允许的动作：

- `give_item`
- `place_blocks`
- `run_command`
- `chat`

如果包含 `place_blocks`，controller 会先保存构建记录，再把同一份方块清单交给 Fabric/Paper 执行并等待后续校验报告。

## 推荐调用方式

1. 先调用 `blockwright_protocol` 理解工具边界。
2. 需要事实时调用读取工具：玩家状态、附近方块、蓝图、构建记录。
3. 需要保存设计时调用 `blockwright_save_blueprint`。
4. 已经知道准确动作时调用 `blockwright_enqueue_actions`。
5. 只有需要兼容自然语言桥时才调用 `blockwright_assistant_message`。
6. Minecraft Fabric/Paper 执行端轮询任务并回写校验报告。
7. 用 `blockwright_get_build` 或 `blockwright_search_builds_nearby` 查看构建记录。

## 后续扩展

下一阶段适合继续补：

- 预览工具：返回待放置体积、冲突、地形融合建议。
- 更完整的构建编辑工具：平移、抬高、重做地基、替换局部材料。
- 更直接的世界读工具：按坐标读取单点/区域方块、读取实体或生物信息。
