---
name: "blockwright-site-selection"
description: "Use for Blockwright placement decisions that must consider the player's facing target, nearby world scan data, ground height, terrain integration, collisions, and tasteful site preparation before building."
---

# Blockwright Site Selection

Use this skill whenever a build or edit depends on the current Minecraft terrain.

## Rules

1. Treat scan data as the current known world state.
2. Treat the scan center and the player's facing/front area as the intended target unless the wider `context_bundle` gives a better reason. Prefer building in front of the player or in a nearby intentional composition instead of relocating to an unrelated empty area.
3. If the requested area has soft blockers such as grass, flowers, snow, mushrooms, or vines, allow clearing them.
4. If every suitable location has hard blockers, assume Blockwright may clear the target volume and continue.
5. Do not refuse the build just because blocks exist in the target area.
6. Place the build with its lowest layer on the first air block above the selected ground surface; do not intentionally float buildings unless the player asks.
7. Keep the saved blueprint and the placed blocks identical.
8. Prefer flat or near-flat footprints. A large build should not rest on one block while the rest hangs in air.
9. Avoid using water, lava, fire, leaves, vines, flowers, tall grass, snow layers, cactus, bamboo, crops, chests, beds, doors, or other fragile/interactive blocks as the supporting surface.
10. If the target surface is not suitable, integrate it into the design: add a deck, stone-brick base, wooden piles, terrace, retaining wall, stairs, bridge, or similar tasteful preparation instead of bluntly rejecting or moving far away.
11. Express your placement decision in `site_plan`: `origin` is the desired world origin, `pre_clear_blocks` are relative air blocks for intentional clearing, `pre_foundation_blocks` are relative support/base blocks, and `rationale` explains the design choice.
12. Keep the entrance reachable from the player's side when the scan/player direction makes that clear.
13. For bridges, docks, treehouses, and other special builds, the structure may span air or water, but the player-facing access point still needs a grounded or otherwise reachable start.
14. Unsuitable terrain is not a reason to reject the request. First adapt the intended target; only relocate nearby when the target is blocked in a way that would clearly damage or contradict the request.
15. For building or edit requests that reference an existing object, inspect the nearest candidate to the player first. Use farther candidates only when the nearest one clearly does not match the requested type or context.
16. If multiple nearby candidates could reasonably match, or the nearest candidate is uncertain, do not place blocks. Ask a concise confirmation question in `reply` with `blueprint=null`, `site_plan=null`, and `actions=[]`.

## Architectural Judgment

- Site selection is a design choice, not just collision avoidance. Prefer a position that makes the building read well from the player's current view.
- Keep the front, entrance, stairs/path, and main decorative face oriented toward the player's side when possible.
- Do not move far away just because a nearby spot is slightly uneven. A small base, deck, bridge, or retaining wall is usually better than losing the player's intended location.
- Let terrain become part of the design when it helps: water can justify a pier or raised platform, slopes can justify stairs and terraces, trees can support a treehouse or shaded build, and sand can justify sandstone paths or a beach-style base.
- The hard limit is safety and impossible ambiguity, not aesthetic preference. If a good builder could reasonably adapt the site, adapt it and continue.

## Player Intent

When the player says to build something, prioritize completing the build. Ask a clarifying question only when the requested target is ambiguous enough that a wrong build would be likely.

For vague target wording, ambiguity is decided around the nearest candidate first. Do not silently choose a farther build because it is easier to match or has cleaner data.

## Practical Placement Preference

When comparing candidate sites, consider:

1. Whether the location serves the player's intent and visual composition.
2. Whether access and entrance direction make sense.
3. Hard-block collision count and clearing amount.
4. Distance from the scan center/player target.
5. Ground support, slope, water, and other terrain integration opportunities.
6. Amount and aesthetics of site preparation.

If no candidate is perfect, choose the least disruptive workable site around the intended target and make it usable. Prefer a small, good-looking terrain adjustment over telling the player the build cannot be placed.

## Follow-up Adjustments

In the same conversation, requests such as "raise it", "move it forward", "fix the foundation", "make it more natural", or "redo the entrance" should be treated as adjustments to the current or matched build. Reconsider the site and produce explicit block changes instead of starting a new unrelated build.
