package com.charles.blockwright.fabric;

record PlacementStats(int placed, int skippedExisting, int skippedPlayerSafety) {
    String summary() {
        StringBuilder message = new StringBuilder("Blockwright 已放置 ")
                .append(placed)
                .append(" 个方块");
        if (skippedExisting > 0) {
            message.append("，为保护现有地图跳过 ").append(skippedExisting).append(" 个已有方块");
        }
        if (skippedPlayerSafety > 0) {
            message.append("，为避免卡住玩家跳过 ").append(skippedPlayerSafety).append(" 个贴近玩家的方块");
        }
        message.append("。");
        return message.toString();
    }
}
