package com.charles.blockwright;

import com.google.gson.annotations.SerializedName;
import java.util.ArrayList;
import java.util.List;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;

public final class JsonModels {
    private JsonModels() {
    }

    public static final class MinecraftMessageRequest {
        @SerializedName("server_id")
        public String serverId;
        public String player;
        public String text;
        public PlayerPosition position;
        @SerializedName("player_state")
        public PlayerState playerState;
        @SerializedName("progress_id")
        public String progressId;
    }

    public static final class MinecraftMessageResponse {
        public String reply;
        public List<GameAction> actions;
        @SerializedName("job_id")
        public String jobId;
    }

    public static final class NextJobResponse {
        public GameJob job;
    }

    public static final class ProgressSnapshot {
        public String id;
        public long sequence;
        public String phase;
        public String detail;
        public String message;
        public boolean done;
        @SerializedName("updated_at_millis")
        public long updatedAtMillis;
    }

    public static final class JobResultRequest {
        public boolean ok;
        public String message;
        public JobExecutionReport report;
        @SerializedName("player_state")
        public PlayerState playerState;
        @SerializedName("nearby_scan")
        public WorldScan nearbyScan;
    }

    public static final class GameJob {
        public String id;
        @SerializedName("server_id")
        public String serverId;
        @SerializedName("target_player")
        public String targetPlayer;
        public String summary;
        public List<GameAction> actions;
    }

    public static final class GameAction {
        public String type;
        public String player;
        public String item;
        public int count;
        public String command;
        public String message;
        public int radius;
        @SerializedName("blueprint_id")
        public String blueprintId;
        public BlockOrigin origin;
        public List<BlueprintBlock> blocks;
        @SerializedName("clear_existing")
        public boolean clearExisting;
    }

    public static final class PlayerPosition {
        public String world;
        public double x;
        public double y;
        public double z;

        public static PlayerPosition fromLocation(Location location) {
            PlayerPosition position = new PlayerPosition();
            position.world = location.getWorld() == null ? "world" : location.getWorld().getName();
            position.x = location.getX();
            position.y = location.getY();
            position.z = location.getZ();
            return position;
        }
    }

    public static final class PlayerState {
        @SerializedName("selected_slot")
        public int selectedSlot;
        @SerializedName("main_hand")
        public PlayerItemStack mainHand;
        @SerializedName("off_hand")
        public PlayerItemStack offHand;
        public List<PlayerInventorySlot> inventory;

        public static PlayerState fromPlayer(Player player) {
            PlayerInventory inventory = player.getInventory();
            PlayerState state = new PlayerState();
            state.selectedSlot = inventory.getHeldItemSlot();
            state.mainHand = itemStack(inventory.getItemInMainHand());
            state.offHand = itemStack(inventory.getItemInOffHand());
            state.inventory = new ArrayList<>();
            for (int slot = 0; slot < inventory.getSize(); slot++) {
                PlayerItemStack item = itemStack(inventory.getItem(slot));
                if (item == null) {
                    continue;
                }
                PlayerInventorySlot inventorySlot = new PlayerInventorySlot();
                inventorySlot.slot = slot;
                inventorySlot.item = item.item;
                inventorySlot.count = item.count;
                inventorySlot.hotbar = slot < 9;
                inventorySlot.selected = slot == state.selectedSlot;
                state.inventory.add(inventorySlot);
            }
            return state;
        }

        private static PlayerItemStack itemStack(ItemStack stack) {
            if (stack == null || stack.getType() == Material.AIR || stack.getAmount() <= 0) {
                return null;
            }
            PlayerItemStack item = new PlayerItemStack();
            item.item = stack.getType().getKey().asString();
            item.count = stack.getAmount();
            return item;
        }
    }

    public static class PlayerItemStack {
        public String item;
        public int count;
    }

    public static final class PlayerInventorySlot extends PlayerItemStack {
        public int slot;
        public boolean hotbar;
        public boolean selected;
    }

    public static final class WorldScan {
        public String world;
        @SerializedName("center_x")
        public int centerX;
        @SerializedName("center_y")
        public int centerY;
        @SerializedName("center_z")
        public int centerZ;
        public int radius;
        public List<WorldScanBlock> blocks;
    }

    public static final class WorldScanBlock {
        public int x;
        public int y;
        public int z;
        public String material;
    }

    public static final class BlockOrigin {
        public String world;
        public int x;
        public int y;
        public int z;
    }

    public static final class BlueprintBlock {
        public int x;
        public int y;
        public int z;
        public String material;
    }

    public static final class JobExecutionReport {
        public List<ActionExecutionReport> actions;

        public boolean isOk() {
            if (actions == null) {
                return true;
            }
            for (ActionExecutionReport action : actions) {
                if (action == null || !"place_blocks".equals(action.actionType)) {
                    continue;
                }
                if (action.mismatchCount > 0 || action.skippedLimitCount > 0) {
                    return false;
                }
                if (action.expectedCount > 0 && action.verifiedCount != action.expectedCount) {
                    return false;
                }
            }
            return true;
        }
    }

    public static final class ActionExecutionReport {
        @SerializedName("action_type")
        public String actionType;
        @SerializedName("blueprint_id")
        public String blueprintId;
        @SerializedName("expected_count")
        public int expectedCount;
        @SerializedName("placed_count")
        public int placedCount;
        @SerializedName("skipped_existing_count")
        public int skippedExistingCount;
        @SerializedName("skipped_limit_count")
        public int skippedLimitCount;
        @SerializedName("skipped_player_safety_count")
        public int skippedPlayerSafetyCount;
        @SerializedName("verified_count")
        public int verifiedCount;
        @SerializedName("mismatch_count")
        public int mismatchCount;
        public List<BlockMismatch> mismatches;
    }

    public static final class BlockMismatch {
        public int x;
        public int y;
        public int z;
        public String expected;
        public String actual;
    }
}
