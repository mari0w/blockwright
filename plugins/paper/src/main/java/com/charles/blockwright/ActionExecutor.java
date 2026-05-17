package com.charles.blockwright;

import java.util.ArrayList;
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
    private static final int MAX_REPORTED_MISMATCHES = 20;

    private final BlockwrightPlugin plugin;

    public ActionExecutor(BlockwrightPlugin plugin) {
        this.plugin = plugin;
    }

    public JsonModels.JobExecutionReport executeActions(
            List<JsonModels.GameAction> actions,
            String defaultPlayer,
            Location fallbackOrigin) {
        JsonModels.JobExecutionReport report = new JsonModels.JobExecutionReport();
        report.actions = new ArrayList<>();

        if (actions == null || actions.isEmpty()) {
            return report;
        }

        for (JsonModels.GameAction action : actions) {
            if (action == null || action.type == null) {
                continue;
            }

            switch (action.type) {
                case "give_item" -> {
                    giveItem(action, defaultPlayer);
                    report.actions.add(nonBlockReport("give_item"));
                }
                case "place_blocks" -> report.actions.add(placeBlocks(action, fallbackOrigin));
                case "chat" -> {
                    sendChat(action, defaultPlayer);
                    report.actions.add(nonBlockReport("chat"));
                }
                default -> {
                    plugin.getLogger().warning("unknown action type: " + action.type);
                    report.actions.add(nonBlockReport(action.type));
                }
            }
        }

        return report;
    }

    private void giveItem(JsonModels.GameAction action, String defaultPlayer) {
        String playerName = action.player != null ? action.player : defaultPlayer;
        Player player = playerName == null ? null : Bukkit.getPlayerExact(playerName);
        if (player == null) {
            throw new IllegalStateException("找不到玩家：" + playerName);
        }

        Material material = itemMaterialFromId(action.item);
        int count = Math.max(action.count, 1);
        player.getInventory().addItem(new ItemStack(material, count));
        player.sendMessage("Blockwright 已发放：" + material.getKey().asString() + " x " + count);
    }

    private JsonModels.ActionExecutionReport placeBlocks(JsonModels.GameAction action, Location fallbackOrigin) {
        JsonModels.ActionExecutionReport report = nonBlockReport("place_blocks");
        report.blueprintId = action.blueprintId;
        report.mismatches = new ArrayList<>();

        if (action.blocks == null || action.blocks.isEmpty()) {
            return report;
        }
        report.expectedCount = action.blocks.size();

        Location origin = resolveOrigin(action.origin, fallbackOrigin);
        World world = origin.getWorld();
        if (world == null) {
            throw new IllegalStateException("放置蓝图失败：世界不存在");
        }

        int placed = 0;
        for (JsonModels.BlueprintBlock blockItem : action.blocks) {
            Material material = blockMaterialFromId(blockItem.material);
            Block block = world.getBlockAt(
                    origin.getBlockX() + blockItem.x,
                    origin.getBlockY() + blockItem.y,
                    origin.getBlockZ() + blockItem.z);
            block.setType(material, false);
            placed++;
        }

        verifyPlacedBlocks(action, origin, world, report);
        report.placedCount = placed;
        return report;
    }

    private void verifyPlacedBlocks(
            JsonModels.GameAction action,
            Location origin,
            World world,
            JsonModels.ActionExecutionReport report) {
        for (JsonModels.BlueprintBlock blockItem : action.blocks) {
            if (blockItem == null) {
                report.mismatchCount++;
                addMismatch(report, origin.getBlockX(), origin.getBlockY(), origin.getBlockZ(), "unknown", "missing_blueprint_block");
                continue;
            }

            int x = origin.getBlockX() + blockItem.x;
            int y = origin.getBlockY() + blockItem.y;
            int z = origin.getBlockZ() + blockItem.z;
            String actual = world.getBlockAt(x, y, z).getType().getKey().asString();
            if (actual.equals(blockItem.material)) {
                report.verifiedCount++;
            } else {
                report.mismatchCount++;
                addMismatch(report, x, y, z, blockItem.material, actual);
            }
        }
    }

    private void addMismatch(
            JsonModels.ActionExecutionReport report,
            int x,
            int y,
            int z,
            String expected,
            String actual) {
        if (report.mismatches.size() >= MAX_REPORTED_MISMATCHES) {
            return;
        }

        JsonModels.BlockMismatch mismatch = new JsonModels.BlockMismatch();
        mismatch.x = x;
        mismatch.y = y;
        mismatch.z = z;
        mismatch.expected = expected;
        mismatch.actual = actual;
        report.mismatches.add(mismatch);
    }

    private JsonModels.ActionExecutionReport nonBlockReport(String actionType) {
        JsonModels.ActionExecutionReport report = new JsonModels.ActionExecutionReport();
        report.actionType = actionType;
        report.expectedCount = 0;
        report.placedCount = 0;
        report.skippedExistingCount = 0;
        report.skippedLimitCount = 0;
        report.verifiedCount = 0;
        report.mismatchCount = 0;
        report.mismatches = new ArrayList<>();
        return report;
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

    private Material itemMaterialFromId(String materialId) {
        Material material = materialFromId(materialId);
        if (!material.isItem()) {
            throw new IllegalArgumentException("不支持的 Minecraft 物品：" + materialId);
        }
        return material;
    }

    private Material blockMaterialFromId(String materialId) {
        Material material = materialFromId(materialId);
        if (!material.isBlock()) {
            throw new IllegalArgumentException("不支持的 Minecraft 方块：" + materialId);
        }
        return material;
    }
}
