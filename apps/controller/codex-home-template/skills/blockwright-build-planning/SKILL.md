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
8. Use block states inside `material` when Minecraft needs them, for example `minecraft:oak_leaves[persistent=true]`, `minecraft:oak_door[half=lower,facing=south]`, and `minecraft:red_bed[part=foot,facing=north]`.

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

## Output Requirements

- Return only the JSON object required by Blockwright.
- Do not explain outside JSON.
- Do not ask the player to install a mod, create a new world, or run a separate server.
- Assume the controller will place the blueprint into the current local/LAN world.
