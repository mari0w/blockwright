package com.charles.blockwright.fabric;

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

    static boolean needsWorldScan(String text) {
        if (text == null || text.isBlank()) {
            return false;
        }

        String lower = text.toLowerCase();
        boolean modifies = text.contains("改")
                || text.contains("换")
                || text.contains("调整")
                || text.contains("替换")
                || lower.contains("modify")
                || lower.contains("replace");
        boolean referencesNearby = text.contains("面前")
                || text.contains("附近")
                || text.contains("这个")
                || text.contains("那栋")
                || text.contains("房子")
                || text.contains("建筑")
                || lower.contains("nearby")
                || lower.contains("this");
        return (modifies && referencesNearby) || isBuildRequest(text, lower);
    }

    private static boolean isBuildRequest(String text, String lower) {
        return text.contains("盖")
                || text.contains("建造")
                || text.contains("修建")
                || text.contains("搭建")
                || text.contains("造一个")
                || text.contains("做一个")
                || text.contains("生成")
                || text.contains("房子")
                || text.contains("木屋")
                || text.contains("树屋")
                || text.contains("房间")
                || text.contains("建筑")
                || text.contains("城堡")
                || text.contains("塔")
                || text.contains("桥")
                || lower.contains("build")
                || lower.contains("house")
                || lower.contains("treehouse")
                || lower.contains("tree house")
                || lower.contains("room")
                || lower.contains("castle")
                || lower.contains("tower")
                || lower.contains("bridge");
    }

    private static String join(String[] args, int start) {
        return String.join(" ", Arrays.copyOfRange(args, start, args.length)).trim();
    }
}
