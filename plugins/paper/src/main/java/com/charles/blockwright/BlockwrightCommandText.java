package com.charles.blockwright;

import java.util.Arrays;

final class BlockwrightCommandText {
    private BlockwrightCommandText() {
    }

    static boolean isReload(String[] args) {
        return args.length == 1 && args[0].equalsIgnoreCase("reload");
    }

    static String extractChatText(String[] args) {
        if (args.length == 0) {
            return null;
        }

        String first = args[0].toLowerCase();
        if (first.equals("ask") || first.equals("chat") || first.equals("say")) {
            if (args.length < 2) {
                return null;
            }
            return join(args, 1);
        }

        if (first.equals("reload")) {
            return null;
        }

        return join(args, 0);
    }

    static String usage() {
        return "用法：/bw <你想要的物品或建筑>，或 /bw ask <需求>";
    }

    private static String join(String[] args, int start) {
        return String.join(" ", Arrays.copyOfRange(args, start, args.length)).trim();
    }
}
