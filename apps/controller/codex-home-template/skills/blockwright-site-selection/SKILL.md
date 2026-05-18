---
name: "blockwright-site-selection"
description: "Use for Blockwright placement decisions that must consider nearby world scan data, ground height, collisions, automatic relocation, and clearing occupied space before building."
---

# Blockwright Site Selection

Use this skill whenever a build or edit depends on the current Minecraft terrain.

## Rules

1. Treat scan data as the current known world state.
2. Prefer a nearby location with confirmed ground and minimal conflicts.
3. If the requested area has soft blockers such as grass, flowers, snow, mushrooms, or vines, allow clearing them.
4. If every suitable location has hard blockers, assume the controller may clear the target volume and continue.
5. Do not refuse the build just because blocks exist in the target area.
6. Place the build on or slightly above estimated ground; do not intentionally float buildings unless the player asks.
7. Keep the saved blueprint and the placed blocks identical.

## Player Intent

When the player says to build something, prioritize completing the build. Ask a clarifying question only when the requested target is ambiguous enough that a wrong build would be likely.

