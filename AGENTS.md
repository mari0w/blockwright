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
- `give_item` 的完成标准是物品进入目标玩家背包，并切换到玩家主手可见的快捷栏槽位；快捷栏/背包满时也要优先把新物品放到主手，旧手持物或多余数量没有存放空间时掉落在玩家脚边，不能只回复“已给”但不让玩家手上拿到。
- 玩家手持物、背包、附近方块、蓝图和构建记录这类读取需求要沉淀成 Blockwright MCP 工具；玩家状态和世界扫描由 Fabric/Paper 通过服务端 API 读取，controller/MCP 对外提供工具语义，不能靠聊天文案或建筑动作绕路。
- 建筑任务必须先在 controller 保存构建记录，再把同一份方块清单下发到 Minecraft；执行端必须逐块读取世界状态生成校验报告，只有报告和构建记录一致才算成功。
- 建筑规划必须考虑 Minecraft 可玩性和方块特性：住宅/木屋/房间/树屋默认要有地板、墙、屋顶、入口、两格高室内空间、床、照明、窗户和可达路径；门要上下两格状态匹配，床要 head/foot 两块匹配，树叶优先使用 `persistent=true` 避免凋零。
- 蓝图 `material` 允许携带方块状态，例如 `minecraft:oak_leaves[persistent=true]`、`minecraft:oak_door[half=lower,facing=south]`；执行端和校验端必须把这些状态视为一致性的一部分。
- 改造现有建筑时必须先扫描玩家附近世界方块，再用扫描结果匹配 `data/builds` 里的构建记录；匹配不到、匹配多个或目标部位不明确时只能追问，不能直接改。
