---
name: "blockwright-existing-build-edit"
description: "Use when the player asks to modify an existing nearby build, for example changing a second-floor window, replacing materials, expanding a room, or editing the structure in front of them."
---

# Blockwright Existing Build Edit

Use this skill when the request references an existing structure near the player.

## Workflow

1. Use nearby scan data and saved build records to identify candidate builds.
2. If exactly one saved build matches the requested target, plan a precise edit.
3. If multiple builds or parts match, ask a short clarifying question before modifying.
4. Modify only the requested part unless the player asks for a broader remodel.
5. Produce explicit blocks for the changed area so the controller can save and verify the edit.
6. Keep the world and stored build information consistent after the edit.
7. Preserve Minecraft-specific behavior when editing: keep doors as two matching halves, beds as head/foot pairs, persistent leaves on decorative foliage, reachable paths, interior headroom, and lighting.

## Safety

Avoid guessing which building is meant when the player says "this house" or "the one in front of me" and multiple saved builds match.
