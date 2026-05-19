---
name: "blockwright-existing-build-edit"
description: "Use when the player asks to modify an existing nearby build, for example changing a second-floor window, replacing materials, expanding a room, or editing the structure in front of them."
---

# Blockwright Existing Build Edit

Use this skill when the request references an existing structure near the player.

## Workflow

1. Use nearby scan data, player position, and saved build records to identify candidate builds.
2. Sort candidates by distance to the player or scan center first, then by how well they match the requested type or part.
3. Prefer the nearest clearly matching build. Do not skip a nearby plausible target in favor of a farther one only because the farther record is cleaner.
4. If exactly one nearby saved build clearly matches the requested target, plan a precise edit.
5. If multiple nearby builds or parts match, or the nearest candidate is plausible but not certain, ask a short clarifying question before modifying.
6. Modify only the requested part unless the player asks for a broader remodel.
7. Produce explicit blocks for the changed area so Blockwright can save and verify the edit.
8. Keep the world and stored build information consistent after the edit.
9. Preserve Minecraft-specific behavior when editing: keep doors as two matching halves, beds as head/foot pairs, persistent leaves on decorative foliage, reachable paths, interior headroom, and lighting.
10. Preserve the existing building's ground contact and access path. Do not move an edit upward, bury it, or leave a door/window/floor floating unless the player asked for that.
11. When expanding a structure, place the new footprint on a reasonable adjacent surface and keep it connected to the existing entrance or interior route.
12. Follow-up requests such as raising, lowering, shifting, reworking the foundation, or making the site look more natural should modify the matched/current build, preserving the player's original target area unless they explicitly ask to relocate it.
13. Use the `recent_builds`, `available_blueprints`, `nearby_scan`, and `scan_analysis` entries from `context_bundle` as the main data source for matching and planning.
14. When an edit needs world placement, output `site_plan` instead of assuming the controller will infer the creative intent.

## Safety

Avoid guessing which building is meant when the player says "this house" or "the one in front of me" and multiple saved builds match. If the nearest candidate is uncertain, reply with a confirmation question and no Minecraft actions.
