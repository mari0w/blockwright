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
    void keepsBlockLimitUncappedForLegacyConfig() {
        assertEquals(0, PlacementPolicy.normalizeMaxBlocks(0));
        assertEquals(5000, PlacementPolicy.normalizeMaxBlocks(5000));
        assertEquals(100_000, PlacementPolicy.normalizeMaxBlocks(100_000));
    }

    @Test
    void playerSafetyZoneCoversBodyAndNearbyEscapeSpace() {
        assertTrue(ActionExecutor.isWithinPlayerSafetyZone(10, 64, 10, 10, 64, 10));
        assertTrue(ActionExecutor.isWithinPlayerSafetyZone(11, 65, 10, 10, 64, 10));
        assertFalse(ActionExecutor.isWithinPlayerSafetyZone(12, 64, 10, 10, 64, 10));
        assertFalse(ActionExecutor.isWithinPlayerSafetyZone(10, 67, 10, 10, 64, 10));
        assertFalse(ActionExecutor.isWithinPlayerSafetyZone(10, 63, 10, 10, 64, 10));
    }
}
