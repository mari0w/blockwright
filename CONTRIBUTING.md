# 贡献指南

感谢你关注 Blockwright。项目当前处于早期阶段，贡献重点是把 HMCL/Fabric 本地世界的闭环做稳定，再逐步扩展更多规划和聊天入口。

## 开发边界

- Minecraft 执行逻辑放在 `plugins/fabric` 和 `plugins/paper`。
- 智能规划、蓝图管理、任务队列、聊天工具和 Codex CLI 入口放在 `apps/controller`。
- 不要把外部机器人、图片分析或 Codex CLI 调用塞进 Minecraft 插件。
- 蓝图方块坐标必须使用相对坐标。
- 建筑任务必须先在 controller 保存构建记录，再下发同一份方块清单。
- 执行端必须逐块读取世界状态生成校验报告。
- 面向玩家和运营的说明文字使用中文。

## 本地环境

需要准备：

- Rust stable。
- JDK 21。
- Gradle。
- Minecraft 1.21.8 + Fabric Loader。
- Fabric API。
- 可选：`cargo-llvm-cov`，用于本地覆盖率门禁。
- 可选：Codex CLI。

启动 controller：

```bash
cp .env.example .env
cargo run -p blockwright-controller
```

安装 Fabric 模组到默认 `.minecraft`：

```bash
make
```

自定义 HMCL 游戏目录：

```bash
make HMCL_DIR=<HMCL当前游戏目录>
```

## 测试要求

新增业务逻辑必须补测试。

常用检查：

```bash
cargo test --workspace
cd plugins/fabric && gradle test
cd plugins/paper && gradle test
```

controller 覆盖率门禁：

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --all-targets --ignore-filename-regex 'apps/controller/src/main.rs' --fail-under-lines 80
```

全量本地测试：

```bash
make test
```

## 代码风格

- Rust 代码提交前运行 `cargo fmt`。
- Java/Kotlin/Gradle 代码尽量沿用现有插件结构。
- 复杂业务分支优先写中文注释。
- 不为“智能”提前做过重抽象；第一阶段优先跑通可验证闭环。
- 不引入真实 token、webhook、client secret 或本地私有配置。
- 面向玩家的 Web 文案需要同步维护中英文；新增页面文字优先接入现有语言字典。
- 文档新增用户入口时，优先同时更新中文 README 和 `README.en.md`。

## Pull Request 要求

PR 描述应包含：

- 改动目的。
- 影响范围。
- 测试结果。
- 是否影响 Minecraft 执行、蓝图格式、构建记录或聊天工具凭证。

如果改动涉及玩家可见行为，请附上中文说明或截图。

## 发布前检查

- `git status` 确认没有误提交 `data/`、`.env`、`config/chat.local.yaml` 或构建产物。
- controller 测试通过。
- Fabric/Paper 相关测试通过。
- README 或 docs 与行为变更同步。
