package com.charles.blockwright.fabric;

final class PlacementPolicy {
    private PlacementPolicy() {
    }

    static boolean canPlace(boolean occupied, boolean protectExistingBlocks, boolean clearExisting) {
        return clearExisting || !protectExistingBlocks || !occupied;
    }

    static int normalizeMaxBlocks(int value) {
        return Math.max(value, 0);
    }
}
