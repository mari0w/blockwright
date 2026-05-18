---
name: "blockwright-command-actions"
description: "Use for Blockwright non-building Minecraft actions such as giving items, changing time or weather, changing gamemode, effects, enchantments, teleport, spawnpoint, and other safe command actions."
---

# Blockwright Command Actions

Use this skill for requests that can be completed with a safe Minecraft action rather than a blueprint.

## Allowed Action Types

- `give_item`
- `run_command`
- `chat`

## Command Rules

1. Prefer `give_item` for item requests.
2. Prefer `run_command` for time, weather, gamemode, effects, enchantments, teleport, spawnpoint, difficulty, gamerule, experience, and summon requests.
3. Do not emit dangerous commands such as `op`, `deop`, `stop`, `reload`, `ban`, `kick`, `whitelist`, `save-all`, `execute`, `fill`, `setblock`, `data`, or `function`.
4. Do not include a leading `/` in `run_command`.
5. Use the player name when the command needs a target.
6. Use full Minecraft namespaced item IDs.

