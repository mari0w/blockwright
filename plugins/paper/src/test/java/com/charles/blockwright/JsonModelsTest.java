package com.charles.blockwright;

import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import org.junit.jupiter.api.Test;

final class JsonModelsTest {
    @Test
    void jobExecutionReportIsOkWithPlacementReport() {
        JsonModels.ActionExecutionReport action = new JsonModels.ActionExecutionReport();
        action.actionType = "place_blocks";
        action.expectedCount = 2;
        action.verifiedCount = 2;
        action.mismatchCount = 0;

        JsonModels.JobExecutionReport report = new JsonModels.JobExecutionReport();
        report.actions = List.of(action);

        assertTrue(report.isOk());
    }

    @Test
    void jobExecutionReportDoesNotFailOnPlacementMismatch() {
        JsonModels.ActionExecutionReport action = new JsonModels.ActionExecutionReport();
        action.actionType = "place_blocks";
        action.expectedCount = 2;
        action.verifiedCount = 1;
        action.mismatchCount = 1;

        JsonModels.JobExecutionReport report = new JsonModels.JobExecutionReport();
        report.actions = List.of(action);

        assertTrue(report.isOk());
    }
}
