package com.charles.blockwright;

import java.util.Set;

final class CommandPolicy {
    private static final Set<String> ALLOWED_ROOTS = Set.of(
            "time",
            "weather",
            "difficulty",
            "gamerule",
            "gamemode",
            "effect",
            "enchant",
            "experience",
            "xp",
            "tp",
            "teleport",
            "spawnpoint",
            "setworldspawn",
            "summon");

    private CommandPolicy() {
    }

    static String normalize(String command) {
        if (command == null) {
            return "";
        }
        String normalized = command.trim();
        while (normalized.startsWith("/")) {
            normalized = normalized.substring(1).trim();
        }
        return normalized.replaceAll("\\s+", " ");
    }

    static boolean isAllowed(String command) {
        if (command == null
                || command.contains("\n")
                || command.contains("\r")
                || command.contains(";")
                || command.contains("&&")
                || command.contains("||")) {
            return false;
        }

        String normalized = normalize(command);
        if (normalized.isBlank()) {
            return false;
        }

        String root = normalized.split("\\s+", 2)[0].toLowerCase();
        return ALLOWED_ROOTS.contains(root);
    }
}
