package com.charles.blockwright.fabric;

import java.util.Locale;
import net.minecraft.server.command.ServerCommandSource;
import net.minecraft.server.network.ServerPlayerEntity;

enum BlockwrightLanguage {
    ENGLISH,
    CHINESE;

    static BlockwrightLanguage fromLanguageCode(String code) {
        if (code == null || code.isBlank()) {
            return ENGLISH;
        }
        return code.toLowerCase(Locale.ROOT).startsWith("zh") ? CHINESE : ENGLISH;
    }

    static BlockwrightLanguage fromPlayer(ServerPlayerEntity player) {
        if (player == null) {
            return ENGLISH;
        }
        return fromLanguageCode(player.getClientOptions().language());
    }

    static BlockwrightLanguage fromSource(ServerCommandSource source) {
        return source == null ? ENGLISH : fromPlayer(source.getPlayer());
    }

    String text(String english, String chinese) {
        return this == CHINESE ? chinese : english;
    }
}
