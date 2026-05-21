---
name: "blockwright-blueprint-verification"
description: "Use for Blockwright consistency requirements: saved blueprint and planned placement actions must describe the same blocks before execution."
---

# Blockwright Blueprint Consistency

Use this skill for every Blockwright action that creates or changes blocks.

## Consistency Contract

1. The blueprint is the source of truth.
2. Blockwright saves the blueprint before sending placement actions.
3. The placement action must use the same `blocks` list that was saved.
4. Blocks use relative coordinates inside the blueprint.
5. `material` may include Minecraft block states, and those states are part of the source of truth.
6. The execution side adds the origin only when placing into the Minecraft world.
7. The selected origin is the first air layer above the site surface. A normal blueprint should therefore have its lowest build layer at relative `y=0`.
8. If a `site_plan` is present, its helper blocks are explicit planned actions too. They should be consistent with the saved blueprint's intended placement and should not create a second hidden build representation.
9. Post-placement world block checking is not a success gate. Once the blueprint and placement actions are saved and sent, the build record follows the execution result instead of re-checking every block.

## Model Behavior

Do not invent a separate representation of the build after the blueprint is created. If the request requires edits, produce a new explicit block plan that can be saved and placed.
