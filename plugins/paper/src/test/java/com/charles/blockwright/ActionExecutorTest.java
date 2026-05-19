package com.charles.blockwright;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

final class ActionExecutorTest {
    @Test
    void handSlotSelectionPrefersCurrentHandWhenStackable() {
        assertEquals(4, ActionExecutor.chooseHandSlot(4, true, 2, 0, 9));
    }

    @Test
    void handSlotSelectionUsesVisibleHotbarBeforeStashingCurrentHand() {
        assertEquals(2, ActionExecutor.chooseHandSlot(4, false, 2, 0, 9));
        assertEquals(0, ActionExecutor.chooseHandSlot(4, false, -1, 0, 9));
    }

    @Test
    void handSlotSelectionReusesCurrentHandWhenHotbarIsFull() {
        assertEquals(4, ActionExecutor.chooseHandSlot(4, false, -1, -1, 9));
        assertEquals(4, ActionExecutor.chooseHandSlot(4, false, -1, -1, -1));
    }
}
