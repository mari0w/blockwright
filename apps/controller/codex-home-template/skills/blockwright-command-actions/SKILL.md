---
name: "blockwright-command-actions"
description: "Use for Blockwright non-building Minecraft actions such as giving items, equipping armor, changing time or weather, changing gamemode, effects, enchantments, teleport, spawnpoint, and other Minecraft command actions."
---

# Blockwright Command Actions

Use this skill for requests that can be completed with an item action or a Minecraft command rather than a blueprint.

## Allowed Action Types

- `give_item`
- `run_command`
- `chat`

## MCP Read Tools

- `blockwright_get_player_state`
- `blockwright_scan_nearby_blocks`
- `blockwright_give_item`
- `blockwright_place_blocks`
- `blockwright_run_command`
- `blockwright_send_chat`
- `blockwright_list_blueprints`
- `blockwright_get_blueprint`
- `blockwright_save_blueprint`
- `blockwright_delete_blueprint`
- `blockwright_list_builds`
- `blockwright_get_build`
- `blockwright_delete_build`
- `blockwright_search_builds_nearby`
- `blockwright_enqueue_actions`

## Command Rules

1. Prefer `give_item` for item requests.
2. Prefer `run_command` for any explicit Minecraft command operation, including time, weather, gamemode, effects, enchantments, teleport, spawnpoint, difficulty, gamerule, experience, summon, armor equip, op, execute, fill, setblock, item, data, function, reload, stop, and other Minecraft commands.
3. A leading `/` is accepted but not required; the execution plugin strips it before dispatch.
4. Use the player name when the command needs a target.
5. Use full Minecraft namespaced item IDs.

## State And Query Contract

- If the player asks what they are holding, whether an item is in the inventory, or what slot currently contains something, call `blockwright_get_player_state`. Do not say the system cannot read it unless that MCP tool returns an explicit failure.
- If the player asks what blocks are nearby, asks to inspect an area, or asks for a radius scan, call `blockwright_scan_nearby_blocks` with the requested radius.
- If the player asks about saved blueprints or prior builds, use the blueprint/build MCP tools instead of guessing from conversation text.
- If the player asks for an item and you are calling MCP directly, use `blockwright_give_item`. If you are returning controller protocol JSON, emit a `give_item` action.
- If the player asks to set or place explicit blocks at known coordinates and you are calling MCP directly, use `blockwright_place_blocks`.
- If the player asks for a Minecraft command operation and you are calling MCP directly, use `blockwright_run_command`.
- If you already know the exact controlled action data, use `blockwright_enqueue_actions` instead of wrapping it in another natural-language request.
- Reading player state or nearby blocks is a tool query, not a building operation and not an item delivery action.

## Item Delivery Contract

- When a player asks for an item, emit a real `give_item` action. Do not only reply with `chat`.
- `give_item` means the execution plugin must put the requested item into the target player's inventory and make the player visibly hold it in the selected main-hand hotbar slot.
- Treat item delivery as a best-effort action, not a wording problem. Do not ask the player to rephrase an item request just because the inventory or hand may be occupied.
- If the target hand or hotbar is occupied, the execution side must still prioritize putting the requested item in the main hand: stack with existing items when possible, use an empty hotbar slot when possible, otherwise replace the selected hand slot. If there is no storage room for displaced or extra items, drop them at the player's feet instead of failing the request.
- The target player comes from the current Minecraft message unless the request names a different online player.
- The reply may say the item was given only when the response includes the matching `give_item` action.
