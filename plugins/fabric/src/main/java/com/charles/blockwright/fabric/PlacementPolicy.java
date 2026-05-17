package com.charles.blockwright.fabric;

final class PlacementPolicy {
    private PlacementPolicy() {
    }

    static boolean canPlace(boolean occupied, boolean protectExistingBlocks) {
        return !protectExistingBlocks || !occupied;
    }

    static int normalizeMaxBlocks(int value) {
        if (value < 1) {
            return 1;
        }
        return Math.min(value, 50_000);
    }
}
