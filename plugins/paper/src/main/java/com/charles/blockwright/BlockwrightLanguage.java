package com.charles.blockwright;

import java.util.Locale;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;

enum BlockwrightLanguage {
    ENGLISH,
    CHINESE;

    static BlockwrightLanguage fromLocale(Locale locale) {
        if (locale == null || locale.getLanguage() == null) {
            return ENGLISH;
        }
        return locale.getLanguage().equalsIgnoreCase("zh") ? CHINESE : ENGLISH;
    }

    static BlockwrightLanguage fromPlayer(Player player) {
        return player == null ? ENGLISH : fromLocale(player.locale());
    }

    static BlockwrightLanguage fromSender(CommandSender sender) {
        return sender instanceof Player player ? fromPlayer(player) : ENGLISH;
    }

    String text(String english, String chinese) {
        return this == CHINESE ? chinese : english;
    }
}
