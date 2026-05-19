package com.charles.blockwright;

import com.google.gson.annotations.SerializedName;
import java.util.List;
import org.bukkit.Location;

public final class JsonModels {
    private JsonModels() {
    }

    public static final class MinecraftMessageRequest {
        @SerializedName("server_id")
        public String serverId;
        public String player;
        public String text;
        public PlayerPosition position;
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
        public String message;
        @SerializedName("blueprint_id")
        public String blueprintId;
        public BlockOrigin origin;
        public List<BlueprintBlock> blocks;
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
                if (action == null) {
                    continue;
                }
                if ("place_blocks".equals(action.actionType)
                        && (action.mismatchCount > 0 || action.verifiedCount != action.expectedCount)) {
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
