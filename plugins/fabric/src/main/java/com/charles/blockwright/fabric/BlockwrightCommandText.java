package com.charles.blockwright.fabric;

import java.util.Arrays;

final class BlockwrightCommandText {
    private BlockwrightCommandText() {
    }

    static String extractChatText(String[] args) {
        if (args.length == 0) {
            return null;
        }

        if (args.length == 1 && args[0].equalsIgnoreCase("web")) {
            return null;
        }

        return join(args, 0);
    }

    static String usage() {
        return usage(BlockwrightLanguage.ENGLISH);
    }

    static String usage(BlockwrightLanguage language) {
        return language.text(
                "Usage: /bw <request>, or /bw web",
                "用法：/bw <需求>，或 /bw web");
    }

    private static String join(String[] args, int start) {
        return String.join(" ", Arrays.copyOfRange(args, start, args.length)).trim();
    }
}
