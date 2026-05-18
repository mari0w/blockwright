# 安全策略

Blockwright 当前处于早期开发阶段，默认面向本地 Minecraft 和局域网使用，不建议直接暴露到公网。

## 支持范围

当前安全维护重点：

- controller HTTP API 鉴权。
- 聊天工具凭证保护。
- Minecraft 命令白名单。
- 构建任务和蓝图执行边界。
- 防止真实密钥、webhook、client secret 或 token 进入仓库。

## 报告安全问题

如果发现安全问题，请优先通过 GitHub Security Advisory 或维护者私下渠道报告，不要先公开利用细节。

报告时请尽量包含：

- 影响版本或 commit。
- 复现步骤。
- 可能影响范围。
- 是否涉及凭证泄露、远程命令、任意方块修改或越权 API 调用。

## 本地部署建议

- 只在可信本机或局域网运行 controller。
- 跨机器访问时启用 `security.require_token`。
- 使用强随机 `shared_token`。
- 不把 `.env`、`config/chat.local.yaml`、`config/secrets/` 或任何真实凭证提交到 Git。
- 不在本地 Minecraft 场景启用 webhook-only 聊天入口。

## 许可证提醒

仓库正式公开前应补充明确的 `LICENSE` 文件。未声明许可证前，外部用户默认没有复制、修改或分发授权。
