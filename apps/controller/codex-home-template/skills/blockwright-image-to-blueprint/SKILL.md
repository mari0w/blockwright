---
name: "blockwright-image-to-blueprint"
description: "Use when the player sends or references an image and wants Blockwright to recreate it as a Minecraft blueprint."
---

# Blockwright Image To Blueprint

Use this skill for image-based recreation requests. When the player sends an
image, the default intent is a one-to-one Minecraft recreation of the visible
object, not a small reference model, not a simplified miniature, and not a loose
style inspiration, unless the player explicitly asks for a smaller or simplified
version.

## Workflow

1. Inspect the image before planning: identify the object type, full visible
   silhouette, width, height, depth cues, material zones, repeated details,
   openings, decoration, and any functional parts.
2. Estimate the Minecraft scale from the actual visual volume and proportions.
   If the image clearly shows a large or detailed object, choose a large
   blueprint and enough blocks to preserve it. Do not minimize the build for
   convenience.
3. Recreate the visible object as completely as block resolution allows:
   preserve the main mass, thickness, depth, front/back/side treatment,
   roof/top, supports, windows, limbs, ears, eyes, decorations, trim, and other
   recognizable features.
4. Map image materials to common vanilla Minecraft blocks while keeping visible
   color and texture zones distinct.
5. Only simplify details that are physically impossible at Minecraft block
   resolution. Never replace the image with a tiny token, thumbnail, flat facade,
   or generic concept build when a fuller recreation is possible.
6. Generate a blueprint with relative coordinates.
7. Choose the scale based on the image, user text, and site data. Do not shrink
   the idea just to fit a preset block budget; make the image-based build as
   complete and large as the player request implies.
8. Ensure `materials` matches `blocks`.
9. Preserve Minecraft playability when the image looks like a usable building:
   entrance, interior headroom, bed or core furniture, lighting, windows, and a
   reachable path.
10. Use explicit block states for special blocks. Leaves should use
   `persistent=true`; doors and beds should include their upper/lower or
   head/foot states.
11. Convert the image into an origin-safe blueprint: the lowest normal
   floor/foundation should start at relative `y=0`, so Blockwright can place it
   on a real ground surface.
12. If the image shows a floating or cliffside structure, include a believable
   support/access path instead of leaving the room unreachable.
13. When the provided site is irregular, adapt the image-based build to the
   player-facing terrain with a deck, terrace, piles, stairs, or base rather than
   refusing the request.
14. If site data is present, output `site_plan` to capture the intended
   placement, support, clearing, and terrain integration for the image-based
   build.

## Recreation Requirements

- Image requests mean recreation by default. Treat words like "copy",
  "recreate", "same as image", "according to this picture", or similar wording
  as a request for a faithful build.
- The build must not be toy-scale for an obviously large or detailed reference.
  A large animal, statue, house, building, vehicle, or scene should use enough
  width, height, depth, and block count to remain recognizable from normal
  Minecraft viewing distance.
- Do not build only the front face when the image implies a three-dimensional
  object. Infer reasonable unseen sides or back from the visible shape and
  material pattern.
- If the image includes many blocks worth of detail, use many blocks. The scale
  is chosen by visual analysis and player intent, not by an artificial maximum.
- If a part is occluded or not visible, infer the most plausible continuation,
  but keep every visible part of the image represented in the blueprint.

## Limits

If image content is not available to the model, explain through a chat action that image analysis is not available for this request. Do not pretend to have inspected an image that was not provided.
