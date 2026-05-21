package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.util.List;
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

    @Test
    void scanControlActionsDoNotFallThroughAsUnsupported() {
        JsonModels.GameAction action = new JsonModels.GameAction();
        action.type = "scan_nearby_and_plan";

        JsonModels.JobExecutionReport report =
                new ActionExecutor(null, new BlockwrightConfig()).executeActions(List.of(action), null);

        assertEquals(1, report.actions.size());
        assertEquals("scan_nearby_and_plan", report.actions.get(0).actionType);
    }
}
