# GitHub Pages 配置说明

[English](GITHUB_PAGES.md) | [简体中文](GITHUB_PAGES.zh-CN.md)

这份说明用于把 Blockwright 仓库配置成“有官网、有 Blog、有 README、有配图”的 GitHub 项目页。仓库已经准备好静态站点入口：

```text
docs/index.html
```

## 仓库 About 区域

建议在 GitHub 仓库右侧 About 里填写：

- Description：`Minecraft Java Edition automation framework with a Rust controller, MCP tools, blueprint records, and Fabric/Paper build verification.`
- Website：发布 Pages 后填写 `https://mari0w.github.io/blockwright/`，如果绑定了自定义域名就填自定义域名。
- Topics：`minecraft`, `fabric`, `rust`, `axum`, `mcp`, `codex`, `ai-agent`, `blueprints`, `java-edition`

## Pages 发布

在 GitHub 仓库页面进入：

```text
Settings -> Pages
```

推荐配置：

- Source：`Deploy from a branch`
- Branch：`main`
- Folder：`/docs`

保存后等待 GitHub Pages 构建完成。按当前 GitHub remote `mari0w/blockwright` 推断，默认地址通常是：

```text
https://mari0w.github.io/blockwright/
```

如果以后要使用自定义域名，再在 Pages 里填域名，并在仓库添加 `docs/CNAME`。现在没有明确域名，所以仓库里不预置 CNAME，避免发布到错误地址。

## README 与官网分工

- `README.md`：默认英文入口，给 GitHub 访问者快速判断项目是什么、怎么跑、怎么安装 Fabric 模组、开发者怎么测试。
- `README.zh-CN.md`：中文 README。
- `docs/index.html`：面向公开展示的项目官网，默认英文，并支持中英文切换。
- `docs/blog.html`、`docs/blog.zh-CN.html`：公开文章列表，用来放发布文章、产品介绍和外部平台稳定回链。
- `docs/articles/`：文章页和 Markdown 草稿。后续新增公开文章时，先放这里，再从 Blog 页加链接。
- `docs/user/JAVA_FABRIC_INSTALL.md`：保留详细安装步骤，README 和官网都链接到这里。
- `docs/ARCHITECTURE.md`、`docs/MCP.md`：给开发者和后续接入者阅读。

## 配图

当前准备了官网和宣传配图：

- `docs/assets/hero-ai-assistant.jpg`：生成的宽幅首页背景图，用在官网首屏。
- `docs/assets/promo-execution.svg`：英文产品图，展示请求入口、模型规划、Minecraft 执行、构建记录和校验。
- `docs/assets/promo-models.svg`：英文产品图，展示支持的 AI 模型后端。
- `docs/assets/promo-commands.svg`：英文产品图，展示命令入口和支持的操作类别。
- `docs/assets/promo-execution.zh-CN.svg`、`docs/assets/promo-models.zh-CN.svg`、`docs/assets/promo-commands.zh-CN.svg`：中文产品图。
- `docs/assets/web-chat-mobile-preview.png`、`docs/assets/web-settings-preview.png`、`docs/assets/web-model-provider-dropdown.png`：旧版 UI 截图，仅保留给内部教程，不作为官网主要宣传图。
- `docs/assets/architecture-flow.svg`：默认英文官网架构图，解释 controller、Codex/MCP 和 Fabric/Paper 执行端之间的关系。
- `docs/assets/architecture-flow.zh-CN.svg`：切换到中文时使用的官网架构图。
- `docs/assets/social-preview.png`：推荐上传到 GitHub 仓库 Settings 的 Social preview；源文件是 `docs/assets/social-preview.svg`。

后续如果有 Minecraft 内实际建筑完成图，建议新增：

```text
docs/assets/minecraft-build-preview.png
```

然后新增案例区域。这样官网可以在生成的首屏视觉之外，继续展示真实游戏内结果。
