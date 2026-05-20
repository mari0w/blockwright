package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

final class PlacementStatsTest {
    @Test
    void summarizesDirectWorldBlockPlacement() {
        assertEquals("Blockwright 已放置 42 个方块。", new PlacementStats(42, 0, 0).summary());
    }

    @Test
    void summarizesSkippedExisting() {
        assertEquals(
                "Blockwright 已放置 10 个方块，为保护现有地图跳过 3 个已有方块。",
                new PlacementStats(10, 3, 0).summary());
    }

    @Test
    void summarizesSkippedPlayerSafetyBlocks() {
        assertEquals(
                "Blockwright 已放置 10 个方块，为避免卡住玩家跳过 4 个贴近玩家的方块。",
                new PlacementStats(10, 0, 4).summary());
    }
}
