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
6. Keep ordinary blueprints compact, but use up to 2000 blocks when the user explicitly asks for a large, detailed, realistic, or reference-image-based build.
7. Make `materials` match the exact block counts in `blocks`.
8. Use block states inside `material` when Minecraft needs them, for example `minecraft:oak_leaves[persistent=true]`, `minecraft:oak_door[half=lower,facing=south]`, and `minecraft:red_bed[part=foot,facing=north]`.
9. Treat blueprint `y=0` as the first placed layer on top of the selected ground surface. Do not encode absolute world height in the blueprint.
10. Keep the lowest normal floor/foundation at `y=0`; use negative `y` only if the player explicitly asks for underground parts.

## Minecraft Playability Rules

- Residential builds such as houses, cabins, rooms, and treehouses should be usable, not only decorative shells.
- Include a complete floor, walls, roof, walkable entrance, at least two blocks of interior headroom, bed, light source, and basic windows unless the player explicitly asks for only an exterior model.
- Doors must be built as two blocks with matching facing: lower half and upper half.
- Beds must be built as two blocks with matching facing: foot and head, with nearby standing space.
- Treehouses and leaf-heavy builds must avoid leaf decay. Prefer leaves with `persistent=true`; if not using persistent leaves, keep leaves close enough to logs.
- Interior spaces must remain navigable. Do not fill the room with solid blocks, oversized furniture, or decoration that blocks the bed or entrance.
- Add ladders, stairs, slabs, or a similar path for elevated rooms, second floors, and treehouses.
- Use stable lighting such as torches, lanterns, or glowstone inside enclosed spaces.
- Avoid gravity, fluid, fire, redstone, and other special-physics blocks unless the required state and safe placement are clear.
- Plan the entrance so it can connect naturally to the player-facing side of the site. Avoid putting the only door against a wall, cliff, water, or a one-block pit.
- Prefer a compact, supported footprint. If the target terrain is a pit, slope, water edge, or odd surface, make the build feel intentionally integrated with a terrace, deck, bridge, stairs, wooden piles, stone-brick base, or retaining wall.
- Do not create terrain-clearing helper blocks inside the blueprint. The controller handles clearing and placement; the blueprint should describe the final structure.
- Do not reject a build because the current terrain may be imperfect. Assume the controller will prefer the player's facing target and can prepare the site with tasteful clearing/foundation when needed.

## Output Requirements

- Return only the JSON object required by Blockwright.
- Do not explain outside JSON.
- Do not ask the player to install a mod, create a new world, or run a separate server.
- Assume the controller will place the blueprint into the current local/LAN world.
- Assume the controller chooses the player-facing target from scan data, with only small adjustments when needed. Your blueprint should be origin-safe: when placed at that target, it should stand naturally and be usable immediately.

## Follow-up Adjustments

If the player continues in the same conversation with feedback like "raise it", "lower it", "move it left", "fix the base", "make it more beautiful", or "redo this part", treat that as an edit to the current build rather than a fresh unrelated structure. Keep the original site intent unless the player asks to move away.
