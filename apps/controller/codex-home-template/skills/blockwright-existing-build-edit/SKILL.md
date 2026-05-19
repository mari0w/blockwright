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
5. Produce explicit blocks for the changed area so Blockwright can save and verify the edit.
6. Keep the world and stored build information consistent after the edit.
7. Preserve Minecraft-specific behavior when editing: keep doors as two matching halves, beds as head/foot pairs, persistent leaves on decorative foliage, reachable paths, interior headroom, and lighting.
8. Preserve the existing building's ground contact and access path. Do not move an edit upward, bury it, or leave a door/window/floor floating unless the player asked for that.
9. When expanding a structure, place the new footprint on a reasonable adjacent surface and keep it connected to the existing entrance or interior route.
10. Follow-up requests such as raising, lowering, shifting, reworking the foundation, or making the site look more natural should modify the matched/current build, preserving the player's original target area unless they explicitly ask to relocate it.

## Safety

Avoid guessing which building is meant when the player says "this house" or "the one in front of me" and multiple saved builds match.
