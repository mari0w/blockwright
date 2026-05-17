# Blockwright 开发约定

- Minecraft 执行逻辑放在 `plugins/fabric` 和 `plugins/paper`，不要把外部机器人、Codex CLI、图片分析塞进插件。
- 智能规划、蓝图管理、任务队列、机器人入口放在 `apps/controller`。
- 蓝图里的方块坐标统一使用相对坐标，真正放置时再叠加玩家或任务原点。
- 面向玩家/运营的说明文字使用中文，代码里的复杂业务分支也优先写中文注释。
- 第一阶段优先做可跑通的闭环，再逐步替换规划器；不要为了“智能”提前做过重抽象。
- 新增业务逻辑必须同步补单元测试；Rust controller 覆盖率门禁不低于 80%，用 `cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80` 校验。
- 聊天工具接入优先支持本地可用的 `polling`、`stream` 或 `local_command`；本地 Minecraft 场景不要启用 webhook-only 入口。
- 聊天工具真实密钥、Webhook、client secret 放在未追踪的本地配置或环境变量里，仓库只能提交示例配置，不能提交真实 token。
- HMCL/单人存档/局域网开放世界的主安装方式是 `plugins/fabric` 生成的 Fabric 模组；不要要求用户迁移地图到 Paper 服务端。`plugins/paper` 只保留给独立 Paper 服务器场景。
- 建筑执行必须走服务端世界方块 API 放置蓝图方块，不能模拟玩家翻背包、选物品、右键摆放；背包/物品栏只用于 `give_item` 这类发物品动作。
- 建筑任务必须先在 controller 保存构建记录，再把同一份方块清单下发到 Minecraft；执行端必须逐块读取世界状态生成校验报告，只有报告和构建记录一致才算成功。
- 改造现有建筑时必须先扫描玩家附近世界方块，再用扫描结果匹配 `data/builds` 里的构建记录；匹配不到、匹配多个或目标部位不明确时只能追问，不能直接改。
