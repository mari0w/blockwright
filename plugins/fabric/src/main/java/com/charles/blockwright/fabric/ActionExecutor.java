package com.charles.blockwright.fabric;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.block.Block;
import net.minecraft.item.Item;
import net.minecraft.item.ItemStack;
import net.minecraft.registry.Registries;
import net.minecraft.registry.RegistryKey;
import net.minecraft.registry.RegistryKeys;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.network.ServerPlayerEntity;
import net.minecraft.server.world.ServerWorld;
import net.minecraft.text.Text;
import net.minecraft.util.Identifier;
import net.minecraft.util.math.BlockPos;

public final class ActionExecutor {
    private static final int MAX_REPORTED_MISMATCHES = 20;

    private final MinecraftServer server;
    private final BlockwrightConfig config;

    public ActionExecutor(MinecraftServer server, BlockwrightConfig config) {
        this.server = server;
        this.config = config;
    }

    public JsonModels.JobExecutionReport executeActions(
            List<JsonModels.GameAction> actions,
            ServerPlayerEntity defaultPlayer) {
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
                case "place_blocks" -> report.actions.add(placeBlocks(action, defaultPlayer));
                case "chat" -> {
                    sendChat(action, defaultPlayer);
                    report.actions.add(nonBlockReport("chat"));
                }
                default -> {
                    defaultPlayer.sendMessage(Text.literal("Blockwright 暂不支持动作：" + action.type), false);
                    report.actions.add(nonBlockReport(action.type));
                }
            }
        }

        return report;
    }

    private void giveItem(JsonModels.GameAction action, ServerPlayerEntity defaultPlayer) {
        ServerPlayerEntity player = resolvePlayer(action.player, defaultPlayer);
        if (player == null) {
            throw new IllegalStateException("找不到玩家：" + action.player);
        }

        Item item = itemFromId(action.item);
        int count = Math.max(action.count, 1);
        player.getInventory().insertStack(new ItemStack(item, count));
        player.sendMessage(Text.literal("Blockwright 已发放：" + Registries.ITEM.getId(item) + " x " + count), false);
    }

    private JsonModels.ActionExecutionReport placeBlocks(
            JsonModels.GameAction action,
            ServerPlayerEntity defaultPlayer) {
        JsonModels.ActionExecutionReport report = nonBlockReport("place_blocks");
        report.blueprintId = action.blueprintId;
        report.mismatches = new ArrayList<>();

        if (action.blocks == null || action.blocks.isEmpty()) {
            return report;
        }
        report.expectedCount = action.blocks.size();

        JsonModels.BlockOrigin origin = action.origin;
        ServerWorld world = resolveWorld(origin, defaultPlayer.getWorld());
        BlockPos basePos = origin == null || origin.world == null
                ? defaultPlayer.getBlockPos().add(2, 0, 2)
                : new BlockPos(origin.x, origin.y, origin.z);

        int placed = 0;
        int skippedExisting = 0;
        int skippedLimit = 0;

        for (int index = 0; index < action.blocks.size(); index++) {
            JsonModels.BlueprintBlock blockItem = action.blocks.get(index);
            if (blockItem == null) {
                continue;
            }
            if (index >= config.maxBlocksPerAction) {
                skippedLimit++;
                continue;
            }

            Block block = blockFromId(blockItem.material);
            BlockPos targetPos = basePos.add(blockItem.x, blockItem.y, blockItem.z);
            boolean occupied = !world.getBlockState(targetPos).isAir();
            if (!PlacementPolicy.canPlace(occupied, config.protectExistingBlocks, action.clearExisting)) {
                skippedExisting++;
                continue;
            }
            world.setBlockState(targetPos, block.getDefaultState());
            placed++;
        }

        verifyPlacedBlocks(action, world, basePos, report);
        report.placedCount = placed;
        report.skippedExistingCount = skippedExisting;
        report.skippedLimitCount = skippedLimit;
        defaultPlayer.sendMessage(Text.literal(new PlacementStats(placed, skippedExisting, skippedLimit).summary()), false);
        return report;
    }

    private void verifyPlacedBlocks(
            JsonModels.GameAction action,
            ServerWorld world,
            BlockPos basePos,
            JsonModels.ActionExecutionReport report) {
        for (JsonModels.BlueprintBlock blockItem : action.blocks) {
            if (blockItem == null) {
                report.mismatchCount++;
                addMismatch(report, basePos, "unknown", "missing_blueprint_block");
                continue;
            }

            BlockPos targetPos = basePos.add(blockItem.x, blockItem.y, blockItem.z);
            String actual = Registries.BLOCK
                    .getId(world.getBlockState(targetPos).getBlock())
                    .toString();
            if (actual.equals(blockItem.material)) {
                report.verifiedCount++;
            } else {
                report.mismatchCount++;
                addMismatch(report, targetPos, blockItem.material, actual);
            }
        }
    }

    private void addMismatch(
            JsonModels.ActionExecutionReport report,
            BlockPos pos,
            String expected,
            String actual) {
        if (report.mismatches.size() >= MAX_REPORTED_MISMATCHES) {
            return;
        }

        JsonModels.BlockMismatch mismatch = new JsonModels.BlockMismatch();
        mismatch.x = pos.getX();
        mismatch.y = pos.getY();
        mismatch.z = pos.getZ();
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

    private void sendChat(JsonModels.GameAction action, ServerPlayerEntity defaultPlayer) {
        if (action.message == null || action.message.isBlank()) {
            return;
        }
        defaultPlayer.sendMessage(Text.literal(action.message), false);
    }

    private ServerPlayerEntity resolvePlayer(String playerName, ServerPlayerEntity defaultPlayer) {
        if (playerName == null || playerName.isBlank()) {
            return defaultPlayer;
        }
        return server.getPlayerManager().getPlayer(playerName);
    }

    private ServerWorld resolveWorld(JsonModels.BlockOrigin origin, ServerWorld fallbackWorld) {
        if (origin == null || origin.world == null || origin.world.isBlank()) {
            return fallbackWorld;
        }

        Identifier worldId = Identifier.tryParse(origin.world);
        if (worldId == null) {
            return fallbackWorld;
        }

        RegistryKey<net.minecraft.world.World> key = RegistryKey.of(RegistryKeys.WORLD, worldId);
        ServerWorld world = server.getWorld(key);
        return world == null ? fallbackWorld : world;
    }

    private Item itemFromId(String itemId) {
        Identifier id = requireIdentifier(itemId, "物品");
        Item item = Registries.ITEM.get(id);
        if (Registries.ITEM.getId(item).equals(Identifier.of("minecraft", "air"))) {
            throw new IllegalArgumentException("不支持的 Minecraft 物品：" + itemId);
        }
        return item;
    }

    private Block blockFromId(String blockId) {
        Identifier id = requireIdentifier(blockId, "方块");
        Block block = Registries.BLOCK.get(id);
        if (Registries.BLOCK.getId(block).equals(Identifier.of("minecraft", "air"))
                && !id.equals(Identifier.of("minecraft", "air"))) {
            throw new IllegalArgumentException("不支持的 Minecraft 方块：" + blockId);
        }
        return block;
    }

    private Identifier requireIdentifier(String value, String label) {
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException(label + " ID 不能为空");
        }

        Identifier id = Identifier.tryParse(value);
        if (id == null) {
            throw new IllegalArgumentException("非法 " + label + " ID：" + value);
        }
        return id;
    }
}
