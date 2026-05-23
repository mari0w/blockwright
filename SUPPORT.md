# Support

Blockwright is early-stage software for local Minecraft worlds. Please include enough runtime context so maintainers can reproduce issues without guessing.

## Where To Ask

- Bugs: open a GitHub issue with the bug report template.
- Feature ideas: open a GitHub issue with the feature request template.
- Security issues: follow [SECURITY.md](SECURITY.md) and avoid public exploit details.
- Usage questions: start with [README.md](README.md), [README.en.md](README.en.md), and `docs/`.

## Useful Details

For controller issues:

- OS and CPU architecture.
- Rust version.
- `cargo run -p blockwright-controller -- serve` or `./scripts/run-web.sh` output.
- The request path, response body, and relevant controller logs.

For Fabric/HMCL issues:

- Minecraft version, Fabric Loader version, and Fabric API version.
- HMCL game directory if custom.
- Whether the world is single-player or opened to LAN.
- `/bw config` output and relevant game log lines.

For Paper issues:

- Paper server version.
- Plugin version or commit.
- `plugins/paper/config.yml` with tokens removed.
- Server log lines around the failed `/bw` command or job execution.

Never include real tokens, webhooks, client secrets, Matrix access tokens, or private server credentials in public issues.
