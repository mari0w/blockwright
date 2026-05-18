---
name: "blockwright-build-planning"
description: "Use for Blockwright Minecraft building requests such as creating houses, treehouses, rooms, towers, bridges, gardens, farms, or other new structures from natural language."
---

# Blockwright Building Planning

Use this skill when the player asks Blockwright to create a new Minecraft structure.

## Workflow

1. Identify the requested structure, style, scale, and key functional parts.
2. Prefer a small but complete build that can be placed quickly in a local world.
3. Generate a blueprint, not Minecraft commands and not inventory/manual interaction steps.
4. Keep all block coordinates relative to the blueprint origin.
5. Use common vanilla Minecraft block IDs with the `minecraft:` namespace.
6. Keep the first-phase blueprint under 500 blocks unless the controller explicitly allows more.
7. Make `materials` match the exact block counts in `blocks`.

## Output Requirements

- Return only the JSON object required by Blockwright.
- Do not explain outside JSON.
- Do not ask the player to install a mod, create a new world, or run a separate server.
- Assume the controller will place the blueprint into the current local/LAN world.

