package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

final class PlacementStatsTest {
    @Test
    void summarizesDirectWorldBlockPlacement() {
        assertEquals("Blockwright placed 42 blocks.", new PlacementStats(42, 0, 0).summary());
    }

    @Test
    void summarizesDirectWorldBlockPlacementInChineseWhenRequested() {
        assertEquals(
                "Blockwright 已放置 42 个方块。",
                new PlacementStats(42, 0, 0).summary(BlockwrightLanguage.CHINESE));
    }

    @Test
    void summarizesSkippedBlocksInEnglish() {
        assertEquals(
                "Blockwright placed 10 blocks, skipped 3 existing blocks to protect the world, skipped 4 blocks too close to the player.",
                new PlacementStats(10, 3, 4).summary(BlockwrightLanguage.ENGLISH));
    }

    @Test
    void summarizesSkippedExisting() {
        assertEquals(
                "Blockwright placed 10 blocks, skipped 3 existing blocks to protect the world.",
                new PlacementStats(10, 3, 0).summary());
    }

    @Test
    void summarizesSkippedPlayerSafetyBlocks() {
        assertEquals(
                "Blockwright placed 10 blocks, skipped 4 blocks too close to the player.",
                new PlacementStats(10, 0, 4).summary());
    }
}
