---
name: "blockwright-blueprint-verification"
description: "Use for Blockwright consistency requirements: saved blueprint, planned actions, world placement, and verification report must describe the same blocks."
---

# Blockwright Blueprint Verification

Use this skill for every Blockwright action that creates or changes blocks.

## Consistency Contract

1. The blueprint is the source of truth.
2. The controller saves the blueprint before sending placement actions.
3. The placement action must use the same `blocks` list that was saved.
4. Blocks use relative coordinates inside the blueprint.
5. The execution side adds the origin only when placing into the Minecraft world.
6. After placement, the execution side verifies the actual world blocks and reports mismatches.
7. If verification reports missing or mismatched blocks, the build record must be treated as failed or needing repair.

## Model Behavior

Do not invent a separate representation of the build after the blueprint is created. If the request requires edits, produce a new explicit block plan that can be saved and verified.

