package com.charles.blockwright;

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
    void placeBlockReportRequiresFullVerification() {
        JsonModels.ActionExecutionReport action = new JsonModels.ActionExecutionReport();
        action.actionType = "place_blocks";
        action.expectedCount = 2;
        action.verifiedCount = 1;
        action.mismatchCount = 1;

        JsonModels.JobExecutionReport report = new JsonModels.JobExecutionReport();
        report.actions = List.of(action);

        assertEquals(false, report.isOk());

        action.verifiedCount = 2;
        action.mismatchCount = 0;

        assertEquals(true, report.isOk());
    }

    @Test
    void placeBlockReportFailsWhenPlacementLimitSkippedBlocks() {
        JsonModels.ActionExecutionReport action = new JsonModels.ActionExecutionReport();
        action.actionType = "place_blocks";
        action.expectedCount = 2;
        action.verifiedCount = 2;
        action.skippedLimitCount = 1;

        JsonModels.JobExecutionReport report = new JsonModels.JobExecutionReport();
        report.actions = List.of(action);

        assertEquals(false, report.isOk());
    }

    @Test
    void detectsPlaceBlockActionsForChunkedRunner() {
        JsonModels.GameAction action = new JsonModels.GameAction();
        action.type = "place_blocks";

        assertEquals(true, JobPoller.hasPlaceBlocks(List.of(action)));
    }

    @Test
    void playerSafetyZoneCoversHeadroomAroundPlayer() {
        assertEquals(true, ActionExecutor.isWithinPlayerSafetyZone(11, 65, 9, 10, 64, 10));
        assertEquals(false, ActionExecutor.isWithinPlayerSafetyZone(12, 65, 9, 10, 64, 10));
        assertEquals(false, ActionExecutor.isWithinPlayerSafetyZone(10, 67, 10, 10, 64, 10));
    }
}
