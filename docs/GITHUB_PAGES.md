# GitHub Pages Setup

[English](GITHUB_PAGES.md) | [简体中文](GITHUB_PAGES.zh-CN.md)

These notes configure Blockwright as a GitHub project with a website, blog, README, and preview image. The repository already includes a static site entrypoint:

```text
docs/index.html
```

## Repository About

Use the following values in the repository About panel:

- Description: `Minecraft Java Edition automation framework with a Rust controller, MCP tools, blueprint records, and Fabric/Paper build verification.`
- Website: after Pages is published, use `https://mari0w.github.io/blockwright/`; if you later bind a custom domain, use that domain instead.
- Topics: `minecraft`, `fabric`, `rust`, `axum`, `mcp`, `codex`, `ai-agent`, `blueprints`, `java-edition`

## Pages Publishing

Open the repository page on GitHub and go to:

```text
Settings -> Pages
```

Recommended settings:

- Source: `Deploy from a branch`
- Branch: `main`
- Folder: `/docs`

After saving, wait for the GitHub Pages build to finish. Based on the current GitHub remote `mari0w/blockwright`, the default URL is usually:

```text
https://mari0w.github.io/blockwright/
```

If you later use a custom domain, configure it in Pages and add `docs/CNAME`. The repository does not include a CNAME file yet because no final custom domain is configured.

## README and Website Roles

- `README.md`: English default overview for GitHub visitors, with product positioning, supported models, command entrances, Fabric installation, and operation patterns.
- `README.zh-CN.md`: Chinese README for Chinese readers.
- `docs/index.html`: public project website for GitHub Pages, defaulting to English with an English/Chinese language switch.
- `docs/blog.html` and `docs/blog.zh-CN.html`: public article index for launch posts, product notes, and stable links from X, Reddit, Modrinth, CurseForge, and other platforms.
- `docs/articles/`: article pages and Markdown drafts. Add new public posts here, then link them from the blog index.
- `docs/user/JAVA_FABRIC_INSTALL.md`: detailed installation guide, linked from both README and the website.
- `docs/ARCHITECTURE.md` and `docs/MCP.md`: developer-facing architecture and MCP documentation.

## Images

The repository currently includes website and promotional assets:

- `docs/assets/hero-ai-assistant.jpg`: generated wide hero background used by the website landing page.
- `docs/assets/promo-execution.svg`: English product visual for request intake, model planning, Minecraft execution, build records, and verification.
- `docs/assets/promo-models.svg`: English product visual for supported AI model backends.
- `docs/assets/promo-commands.svg`: English product visual for command entrances and supported operation categories.
- `docs/assets/promo-execution.zh-CN.svg`, `docs/assets/promo-models.zh-CN.svg`, `docs/assets/promo-commands.zh-CN.svg`: Chinese versions of the product visuals.
- `docs/assets/web-chat-mobile-preview.png`, `docs/assets/web-settings-preview.png`, `docs/assets/web-model-provider-dropdown.png`: legacy UI captures kept for internal tutorials, not used as the main website promotional visuals.
- `docs/assets/architecture-flow.svg`: English architecture diagram for the default website language.
- `docs/assets/architecture-flow.zh-CN.svg`: Chinese architecture diagram used when the website language is switched to Chinese.
- `docs/assets/social-preview.png`: recommended image for GitHub repository Social preview; the source file is `docs/assets/social-preview.svg`.

When real in-game build captures are available, add:

```text
docs/assets/minecraft-build-preview.png
```

Then use it in a case-study section so the website can show actual Minecraft build results alongside the generated hero visual.
