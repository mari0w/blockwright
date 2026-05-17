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
    }

    public static final class MinecraftMessageResponse {
        public String reply;
        public List<GameAction> actions;
    }

    public static final class NextJobResponse {
        public GameJob job;
    }

    public static final class JobResultRequest {
        public boolean ok;
        public String message;
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
}

