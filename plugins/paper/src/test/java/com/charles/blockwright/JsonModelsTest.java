package com.charles.blockwright;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.Gson;
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
    void jobExecutionReportFailsOnPlacementMismatch() {
        JsonModels.ActionExecutionReport action = new JsonModels.ActionExecutionReport();
        action.actionType = "place_blocks";
        action.expectedCount = 2;
        action.verifiedCount = 1;
        action.mismatchCount = 1;

        JsonModels.JobExecutionReport report = new JsonModels.JobExecutionReport();
        report.actions = List.of(action);

        assertFalse(report.isOk());
    }

    @Test
    void minecraftMessageRequestSerializesNearbyScan() {
        JsonModels.MinecraftMessageRequest request = new JsonModels.MinecraftMessageRequest();
        request.nearbyScan = new JsonModels.WorldScan();
        request.nearbyScan.world = "world";
        request.nearbyScan.centerX = 1;
        request.nearbyScan.centerY = 64;
        request.nearbyScan.centerZ = 2;
        request.nearbyScan.radius = 8;
        JsonModels.WorldScanBlock block = new JsonModels.WorldScanBlock();
        block.x = 1;
        block.y = 63;
        block.z = 2;
        block.material = "minecraft:stone";
        request.nearbyScan.blocks = List.of(block);

        String json = new Gson().toJson(request);

        assertTrue(json.contains("\"nearby_scan\""));
        assertTrue(json.contains("\"center_x\":1"));
        assertTrue(json.contains("\"material\":\"minecraft:stone\""));
    }
}
