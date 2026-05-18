# Blockwright MCP

Blockwright MCP 是 controller 的可选 stdio 入口，用来让 Codex、机器人或其他 MCP 客户端以“助手”方式调用 Blockwright。

它不是裸 Minecraft API MCP。不要通过 MCP 暴露 `setBlock`、`fill`、任意命令或背包模拟操作。MCP 只暴露 Blockwright 的高层能力，真正改世界仍然走 Fabric/Paper 执行端、构建记录和逐块校验。

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
- dry-run 返回计划。
- `execute=true` 时把受控动作入队给 Minecraft 执行端。
- 查询蓝图。
- 查询构建记录。
- 校验蓝图 JSON。

MCP 不做：

- 不直接调用 Minecraft `setBlock` / `fill`。
- 不暴露任意服务端命令。
- 不模拟玩家翻背包、选物品、右键放置。
- 不绕过 controller 的构建记录和执行端校验。

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
  "server_id": "hmcl-lan",
  "target_player": "Steve",
  "execute": false
}
```

执行时：

```json
{
  "text": "给 Steve 一把钻石剑",
  "server_id": "hmcl-lan",
  "target_player": "Steve",
  "execute": true
}
```

### `blockwright_protocol`

返回 Blockwright 的安全动作协议和边界，适合 AI 在规划前读取。

### `blockwright_validate_blueprint`

校验蓝图是否满足基本结构要求，例如：

- 材料是否使用 `minecraft:` 命名空间。
- `materials` 统计是否和 `blocks` 一致。
- 普通蓝图是否错误使用负相对高度。

### 蓝图和构建记录查询

- `blockwright_list_blueprints`
- `blockwright_get_blueprint`
- `blockwright_list_builds`
- `blockwright_get_build`
- `blockwright_health`

## 推荐调用方式

1. 先调用 `blockwright_protocol` 理解边界。
2. 调用 `blockwright_assistant_message`，`execute=false` 看计划。
3. 如果计划合理，再用同类请求设置 `execute=true` 入队。
4. Minecraft Fabric/Paper 执行端轮询任务并回写校验报告。
5. 用 `blockwright_get_build` 查看构建记录。

## 后续扩展

下一阶段适合继续补：

- 玩家上下文工具：读取当前在线玩家、位置和朝向。
- 扫描工具：请求 Fabric/Paper 扫描玩家面前区域。
- 预览工具：返回待放置体积、冲突、地形融合建议。
- 更完整的构建编辑工具：平移、抬高、重做地基、替换局部材料。
