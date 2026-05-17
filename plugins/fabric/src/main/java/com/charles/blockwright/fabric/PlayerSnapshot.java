package com.charles.blockwright.fabric;

import net.minecraft.server.network.ServerPlayerEntity;

public record PlayerSnapshot(String name, JsonModels.PlayerPosition position) {
    public static PlayerSnapshot from(ServerPlayerEntity player) {
        return new PlayerSnapshot(player.getName().getString(), JsonModels.PlayerPosition.fromPlayer(player));
    }
}
