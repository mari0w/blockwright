---
name: "blockwright-build-planning"
description: "Use for Blockwright Minecraft building requests such as creating houses, treehouses, rooms, towers, bridges, gardens, farms, or other new structures from natural language."
---

# Blockwright Building Planning

Use this skill when the player asks Blockwright to create a new Minecraft structure.

## Workflow

1. Identify the requested structure, style, scale, and key functional parts.
2. Treat the requested build as a creative architecture task, not a rigid template fill. Use the player's wording, the current site, and Minecraft playability to choose style, scale, materials, silhouette, entrance, and details.
3. If the player asks for a named new structure, build it directly. Do not ask the player to confirm style, size, location, or approach when a reasonable default can be chosen.
4. Generate a blueprint, not Minecraft commands and not inventory/manual interaction steps.
5. Keep all block coordinates relative to the blueprint origin.
6. Use common vanilla Minecraft block IDs with the `minecraft:` namespace.
7. Keep ordinary blueprints compact, but use up to 5000 blocks when the user asks for a large, detailed, realistic, scenic, or reference-image-based build.
8. Make `materials` match the exact block counts in `blocks`.
9. Use block states inside `material` when Minecraft needs them, for example `minecraft:oak_leaves[persistent=true]`, `minecraft:oak_door[half=lower,facing=south]`, and `minecraft:red_bed[part=foot,facing=north]`.
10. Treat blueprint `y=0` as the first placed layer on top of the selected ground surface. Do not encode absolute world height in the blueprint.
11. Keep the lowest normal floor/foundation at `y=0`; use negative `y` only if the player explicitly asks for underground parts.
12. When the build depends on the current site, output a `site_plan` that states the intended origin, clearing, foundation/support blocks, and rationale. Use `site_plan.origin=null` only when Blockwright should choose an origin from the supplied data.
13. If the wording may refer to an existing nearby build, first inspect the nearest candidate from `recent_builds`, `nearby_scan`, and the player position before creating a fresh unrelated build.
14. If it is unclear whether the player wants a new structure or wants to modify the nearest existing structure, reply with a short confirmation question and return `blueprint=null`, `site_plan=null`, and `actions=[]`. Do not use this confirmation path for obvious new-build requests like "build a creeper building".

## Creative Architecture Rules

- One finished structure should be represented by one blueprint file. The blueprint is the source of truth for that building; later edits should reference or revise that building record instead of inventing an unrelated duplicate.
- Treat blueprint files and build records as data managed through MCP tools. Save or update blueprints through the blueprint tools; search nearby builds through build tools before editing an existing structure.
- Existing blueprint files are reference material, not a cage. Reuse them when they match the request, but feel free to create a new variation when the player asks for something different or the current scene calls for a better design.
- Do not overfit to tiny literal defaults. If the player says "a creeper building", it can be a usable creeper-shaped house, tower, shop, statue-room, or scenic build depending on the site. Pick the version that will look good and be useful.
- Prefer a coherent building concept: recognizable silhouette, readable front side, entrance, interior logic, material palette, roof/top treatment, lighting, and small details.
- Let the site influence the build. Sand, water, slopes, trees, existing paths, cliffs, and open space can change the base, platform, bridge, stair, deck, window direction, or viewing angle.
- Avoid rigid rejection. If the terrain is imperfect, adapt the building with foundation, terrace, stilts, retaining walls, stairs, bridges, or tasteful clearing.
- Keep the design compact enough to execute, but do not make it toy-like when the request implies a landmark or themed building.

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
- Do not create terrain-clearing helper blocks inside the blueprint. Blockwright handles clearing and placement; the blueprint should describe the final structure.
- Do not reject a build because the current terrain may be imperfect. Assume Blockwright will prefer the player's facing target and can prepare the site with tasteful clearing/foundation when needed.

## Output Requirements

- Return only the JSON object required by Blockwright.
- Do not explain outside JSON.
- The top-level object must include exactly the protocol fields `reply`, `summary`, `blueprint`, `site_plan`, and `actions`.
- One building equals one blueprint object and one saved blueprint file. Do not split one coherent building across multiple unrelated blueprint objects.
- Blueprint size must be `size: {"width": ..., "height": ..., "depth": ...}`. Do not use `dimensions`, `origin_mode`, or other aliases.
- If `site_plan` is not `null`, include all fields: `origin`, `clear_existing`, `pre_clear_blocks`, `pre_foundation_blocks`, and `rationale`.
- When returning a `blueprint`, keep `actions` empty unless you need an additional non-placement action. Blockwright will turn the saved blueprint into `place_blocks`; do not output a placeholder `place_blocks` action without `blocks`.
- Player-facing `reply` should say what will be built or done, not ask for confirmation and not say "I need to look first" when scan or site data is already present.
- Do not ask the player to install a mod, create a new world, or run a separate server.
- Assume Blockwright will place the blueprint into the current local/LAN world.
- Treat the supplied `context_bundle` as the data source. Blockwright validates safety, but you own the creative decision for scale, composition, orientation, and site integration.

## Follow-up Adjustments

If the player continues in the same conversation with feedback like "raise it", "lower it", "move it left", "fix the base", "make it more beautiful", or "redo this part", treat that as an edit to the current build rather than a fresh unrelated structure. Keep the original site intent unless the player asks to move away.

When the player uses vague references such as "this", "the building near me", "the one in front", or "my house", prefer the nearest matching saved/scanned build. If the nearest candidate is not clearly the target, ask for confirmation in `reply` instead of guessing.
