package com.charles.blockwright;

import java.util.List;
import java.util.Locale;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;

public final class ActionExecutor {
    private final BlockwrightPlugin plugin;

    public ActionExecutor(BlockwrightPlugin plugin) {
        this.plugin = plugin;
    }

    public void executeActions(List<JsonModels.GameAction> actions, String defaultPlayer, Location fallbackOrigin) {
        if (actions == null || actions.isEmpty()) {
            return;
        }

        for (JsonModels.GameAction action : actions) {
            if (action == null || action.type == null) {
                continue;
            }

            switch (action.type) {
                case "give_item" -> giveItem(action, defaultPlayer);
                case "place_blocks" -> placeBlocks(action, fallbackOrigin);
                case "chat" -> sendChat(action, defaultPlayer);
                default -> plugin.getLogger().warning("unknown action type: " + action.type);
            }
        }
    }

    private void giveItem(JsonModels.GameAction action, String defaultPlayer) {
        String playerName = action.player != null ? action.player : defaultPlayer;
        Player player = playerName == null ? null : Bukkit.getPlayerExact(playerName);
        if (player == null) {
            throw new IllegalStateException("找不到玩家：" + playerName);
        }

        Material material = materialFromId(action.item);
        int count = Math.max(action.count, 1);
        player.getInventory().addItem(new ItemStack(material, count));
        player.sendMessage("Blockwright 已发放：" + material.getKey().asString() + " x " + count);
    }

    private void placeBlocks(JsonModels.GameAction action, Location fallbackOrigin) {
        if (action.blocks == null || action.blocks.isEmpty()) {
            return;
        }

        Location origin = resolveOrigin(action.origin, fallbackOrigin);
        World world = origin.getWorld();
        if (world == null) {
            throw new IllegalStateException("放置蓝图失败：世界不存在");
        }

        for (JsonModels.BlueprintBlock blockItem : action.blocks) {
            Material material = materialFromId(blockItem.material);
            Block block = world.getBlockAt(
                    origin.getBlockX() + blockItem.x,
                    origin.getBlockY() + blockItem.y,
                    origin.getBlockZ() + blockItem.z);
            block.setType(material, false);
        }
    }

    private void sendChat(JsonModels.GameAction action, String defaultPlayer) {
        if (action.message == null || action.message.isBlank()) {
            return;
        }

        Player player = defaultPlayer == null ? null : Bukkit.getPlayerExact(defaultPlayer);
        if (player != null) {
            player.sendMessage(action.message);
        } else {
            Bukkit.broadcastMessage(action.message);
        }
    }

    private Location resolveOrigin(JsonModels.BlockOrigin origin, Location fallbackOrigin) {
        if (origin == null) {
            return fallbackOrigin;
        }

        World world = null;
        if (origin.world != null && !origin.world.isBlank()) {
            world = Bukkit.getWorld(origin.world);
        }
        if (world == null) {
            world = fallbackOrigin.getWorld();
        }
        return new Location(world, origin.x, origin.y, origin.z);
    }

    private Material materialFromId(String materialId) {
        if (materialId == null || materialId.isBlank()) {
            throw new IllegalArgumentException("材质不能为空");
        }

        String normalized = materialId.toUpperCase(Locale.ROOT).replace("MINECRAFT:", "");
        Material material = Material.matchMaterial(normalized);
        if (material == null || !material.isBlock() && !material.isItem()) {
            throw new IllegalArgumentException("不支持的 Minecraft 材质：" + materialId);
        }
        return material;
    }
}
