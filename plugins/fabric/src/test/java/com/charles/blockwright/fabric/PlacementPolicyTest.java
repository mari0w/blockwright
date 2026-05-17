package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

final class PlacementPolicyTest {
    @Test
    void protectsExistingBlocksByDefaultPolicy() {
        assertFalse(PlacementPolicy.canPlace(true, true, false));
        assertTrue(PlacementPolicy.canPlace(false, true, false));
    }

    @Test
    void canAllowOverwriteWhenExplicitlyConfigured() {
        assertTrue(PlacementPolicy.canPlace(true, false, false));
        assertTrue(PlacementPolicy.canPlace(true, true, true));
    }

    @Test
    void normalizesBlockLimitIntoSafeRange() {
        assertEquals(1, PlacementPolicy.normalizeMaxBlocks(0));
        assertEquals(5000, PlacementPolicy.normalizeMaxBlocks(5000));
        assertEquals(50_000, PlacementPolicy.normalizeMaxBlocks(100_000));
    }
}
