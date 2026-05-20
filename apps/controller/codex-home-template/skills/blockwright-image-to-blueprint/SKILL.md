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
5. Choose the scale based on the image, user text, and site data. Do not shrink the idea just to fit a preset block budget; make the image-inspired build as complete and large as the player request implies.
6. Ensure `materials` matches `blocks`.
7. Preserve Minecraft playability when the image looks like a usable building: entrance, interior headroom, bed or core furniture, lighting, windows, and a reachable path.
8. Use explicit block states for special blocks. Leaves should use `persistent=true`; doors and beds should include their upper/lower or head/foot states.
9. Convert the image into an origin-safe blueprint: the lowest normal floor/foundation should start at relative `y=0`, so Blockwright can place it on a real ground surface.
10. If the image shows a floating or cliffside structure, include a believable support/access path instead of leaving the room unreachable.
11. When the provided site is irregular, adapt the image-inspired build to the player-facing terrain with a deck, terrace, piles, stairs, or base rather than refusing the request.
12. If site data is present, output `site_plan` to capture the intended placement, support, clearing, and terrain integration for the image-inspired build.

## Limits

If image content is not available to the model, explain through a chat action that image analysis is not available for this request. Do not pretend to have inspected an image that was not provided.
