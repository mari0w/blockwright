package com.charles.blockwright;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.block.data.BlockData;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;

public final class ActionExecutor {
    private static final int HOTBAR_SIZE = 9;
    private static final int PLAYER_STORAGE_SIZE = 36;
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
                case "run_command" -> {
                    runCommand(action, defaultPlayer);
                    report.actions.add(nonBlockReport("run_command"));
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
        int heldSlot = putItemInMainHand(player, material, count);
        player.updateInventory();
        player.sendMessage(
                "Blockwright 已发放并切到手上：" + material.getKey().asString() + " x " + count
                        + "（快捷栏 " + (heldSlot + 1) + "）");
    }

    private int putItemInMainHand(Player player, Material material, int count) {
        PlayerInventory inventory = player.getInventory();
        ItemStack handStack = new ItemStack(material, Math.min(count, material.getMaxStackSize()));
        int selectedSlot = inventory.getHeldItemSlot();
        int firstStackableHotbarSlot = findStackableHotbarSlot(inventory, handStack);
        int firstEmptyHotbarSlot = findEmptyHotbarSlot(inventory);
        int firstEmptyStorageSlot = findEmptyStorageSlot(inventory);
        int targetSlot = chooseHandSlot(
                selectedSlot,
                canSlotAcceptMore(inventory.getItem(selectedSlot), handStack),
                firstStackableHotbarSlot,
                firstEmptyHotbarSlot,
                firstEmptyStorageSlot);

        ItemStack targetStack = inventory.getItem(targetSlot);
        if (targetSlot == selectedSlot
                && !isEmpty(targetStack)
                && !canSlotAcceptMore(targetStack, handStack)) {
            // 必须优先让新物品到手上；背包也满时，把旧手持物安全掉在玩家脚边。
            if (firstEmptyStorageSlot >= HOTBAR_SIZE) {
                inventory.setItem(firstEmptyStorageSlot, targetStack.clone());
            } else {
                player.getWorld().dropItemNaturally(player.getLocation(), targetStack.clone());
            }
            inventory.setItem(selectedSlot, null);
        }

        int heldCount = moveIntoSlot(inventory, targetSlot, handStack);
        inventory.setHeldItemSlot(targetSlot);
        insertRemaining(player, inventory, material, count - heldCount);
        return targetSlot;
    }

    static int chooseHandSlot(
            int selectedSlot,
            boolean selectedCanAccept,
            int firstStackableHotbarSlot,
            int firstEmptyHotbarSlot,
            int firstEmptyStorageSlot) {
        if (selectedCanAccept && isHotbarSlot(selectedSlot)) {
            return selectedSlot;
        }
        if (isHotbarSlot(firstStackableHotbarSlot)) {
            return firstStackableHotbarSlot;
        }
        if (isHotbarSlot(firstEmptyHotbarSlot)) {
            return firstEmptyHotbarSlot;
        }
        if (isHotbarSlot(selectedSlot)) {
            return selectedSlot;
        }
        return 0;
    }

    private static boolean isHotbarSlot(int slot) {
        return slot >= 0 && slot < HOTBAR_SIZE;
    }

    private int findStackableHotbarSlot(PlayerInventory inventory, ItemStack stack) {
        for (int slot = 0; slot < HOTBAR_SIZE; slot++) {
            if (canSlotAcceptMore(inventory.getItem(slot), stack)) {
                return slot;
            }
        }
        return -1;
    }

    private int findEmptyHotbarSlot(PlayerInventory inventory) {
        for (int slot = 0; slot < HOTBAR_SIZE; slot++) {
            if (isEmpty(inventory.getItem(slot))) {
                return slot;
            }
        }
        return -1;
    }

    private int findEmptyStorageSlot(PlayerInventory inventory) {
        for (int slot = HOTBAR_SIZE; slot < PLAYER_STORAGE_SIZE; slot++) {
            if (isEmpty(inventory.getItem(slot))) {
                return slot;
            }
        }
        return -1;
    }

    private boolean canSlotAcceptMore(ItemStack current, ItemStack stack) {
        return !isEmpty(current)
                && current.isSimilar(stack)
                && current.getAmount() < current.getMaxStackSize();
    }

    private boolean isEmpty(ItemStack stack) {
        return stack == null || stack.getType().isAir() || stack.getAmount() <= 0;
    }

    private int moveIntoSlot(PlayerInventory inventory, int slot, ItemStack stack) {
        ItemStack current = inventory.getItem(slot);
        if (isEmpty(current)) {
            inventory.setItem(slot, stack);
            return stack.getAmount();
        }

        int moved = Math.min(stack.getAmount(), current.getMaxStackSize() - current.getAmount());
        current.setAmount(current.getAmount() + moved);
        inventory.setItem(slot, current);
        return moved;
    }

    private void insertRemaining(Player player, PlayerInventory inventory, Material material, int count) {
        int remaining = count;
        while (remaining > 0) {
            int chunk = Math.min(remaining, material.getMaxStackSize());
            HashMap<Integer, ItemStack> leftovers = inventory.addItem(new ItemStack(material, chunk));
            for (ItemStack leftover : leftovers.values()) {
                if (!isEmpty(leftover)) {
                    player.getWorld().dropItemNaturally(player.getLocation(), leftover);
                }
            }
            remaining -= chunk;
        }
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
            BlockData blockData = blockDataFromId(blockItem.material);
            Block block = world.getBlockAt(
                    origin.getBlockX() + blockItem.x,
                    origin.getBlockY() + blockItem.y,
                    origin.getBlockZ() + blockItem.z);
            block.setBlockData(blockData, false);
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
            BlockData actual = world.getBlockAt(x, y, z).getBlockData();
            if (matchesBlockData(actual, blockItem.material)) {
                report.verifiedCount++;
            } else {
                report.mismatchCount++;
                addMismatch(report, x, y, z, blockItem.material, actual.getAsString(false));
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

    private void runCommand(JsonModels.GameAction action, String defaultPlayer) {
        String command = CommandPolicy.normalize(action.command);
        if (!CommandPolicy.isAllowed(command)) {
            throw new IllegalArgumentException("不允许执行的 Minecraft 指令：" + action.command);
        }

        Bukkit.dispatchCommand(Bukkit.getConsoleSender(), command);
        Player player = defaultPlayer == null ? null : Bukkit.getPlayerExact(defaultPlayer);
        if (player != null) {
            player.sendMessage("Blockwright 已执行指令：/" + command);
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

    private BlockData blockDataFromId(String materialId) {
        try {
            return Bukkit.createBlockData(materialId);
        } catch (IllegalArgumentException error) {
            throw new IllegalArgumentException("不支持的 Minecraft 方块状态：" + materialId, error);
        }
    }

    private boolean matchesBlockData(BlockData actual, String expectedMaterial) {
        if (!expectedMaterial.contains("[")) {
            return actual.getMaterial() == blockMaterialFromId(expectedMaterial);
        }
        return blockDataFromId(expectedMaterial).matches(actual);
    }
}
