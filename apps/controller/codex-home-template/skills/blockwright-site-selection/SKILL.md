---
name: "blockwright-site-selection"
description: "Use for Blockwright placement decisions that must consider the player's facing target, nearby world scan data, ground height, terrain integration, collisions, and tasteful site preparation before building."
---

# Blockwright Site Selection

Use this skill whenever a build or edit depends on the current Minecraft terrain.

## Rules

1. Treat scan data as the current known world state.
2. Treat the scan center as the player's intended target. Prefer building at that target or a very small adjustment around it instead of relocating to an unrelated empty area.
3. If the requested area has soft blockers such as grass, flowers, snow, mushrooms, or vines, allow clearing them.
4. If every suitable location has hard blockers, assume the controller may clear the target volume and continue.
5. Do not refuse the build just because blocks exist in the target area.
6. Place the build with its lowest layer on the first air block above the selected ground surface; do not intentionally float buildings unless the player asks.
7. Keep the saved blueprint and the placed blocks identical.
8. Prefer flat or near-flat footprints. A large build should not rest on one block while the rest hangs in air.
9. Avoid using water, lava, fire, leaves, vines, flowers, tall grass, snow layers, cactus, bamboo, crops, chests, beds, doors, or other fragile/interactive blocks as the supporting surface.
10. If the target surface is not suitable, integrate it into the design: add a deck, stone-brick base, wooden piles, terrace, retaining wall, stairs, bridge, or similar tasteful preparation instead of bluntly rejecting or moving far away.
11. Keep the entrance reachable from the player's side when the scan/player direction makes that clear.
12. For bridges, docks, treehouses, and other special builds, the structure may span air or water, but the player-facing access point still needs a grounded or otherwise reachable start.
13. Unsuitable terrain is not a reason to reject the request. First adapt the intended target; only relocate within a small nearby range when the target is blocked in a way that would clearly damage or contradict the request.

## Player Intent

When the player says to build something, prioritize completing the build. Ask a clarifying question only when the requested target is ambiguous enough that a wrong build would be likely.

## Practical Placement Preference

Score candidate sites in this order:

1. Lowest hard-block collision count.
2. Lowest total clearing amount.
3. Closest point to the scan center/player target.
4. Flattest existing ground.
5. Known safe support under the footprint.
6. Lowest amount of site preparation.

If no candidate is perfect, choose the least disruptive workable site around the intended target and make it usable. Prefer a small, good-looking terrain adjustment over telling the player the build cannot be placed.

## Follow-up Adjustments

In the same conversation, requests such as "raise it", "move it forward", "fix the foundation", "make it more natural", or "redo the entrance" should be treated as adjustments to the current or matched build. Reconsider the site and produce explicit block changes instead of starting a new unrelated build.
