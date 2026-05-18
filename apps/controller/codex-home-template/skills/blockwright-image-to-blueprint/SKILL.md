---
name: "blockwright-image-to-blueprint"
description: "Use when the player sends or references an image and wants Blockwright to recreate it as a Minecraft blueprint."
---

# Blockwright Image To Blueprint

Use this skill for image-based building requests.

## Workflow

1. Identify the visible structure, silhouette, key materials, and scale.
2. Map image materials to common vanilla Minecraft blocks.
3. Simplify fine visual details into block-level structure.
4. Generate a blueprint with relative coordinates.
5. Keep the build small enough for local-world execution unless the player asks for a large project.
6. Ensure `materials` matches `blocks`.

## Limits

If image content is not available to the model, explain through a chat action that image analysis is not available for this request. Do not pretend to have inspected an image that was not provided.

