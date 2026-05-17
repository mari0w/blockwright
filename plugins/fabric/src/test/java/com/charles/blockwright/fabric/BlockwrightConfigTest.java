package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;

final class BlockwrightConfigTest {
    @Test
    void createsDefaultConfigWhenFileIsMissing() throws Exception {
        Path path = Files.createTempDirectory("blockwright-fabric-config").resolve("blockwright.json");

        BlockwrightConfig config = BlockwrightConfig.load(path);

        assertEquals("http://127.0.0.1:8765", config.controllerUrl);
        assertEquals("hmcl-lan", config.serverId);
        assertEquals(5, config.connectTimeoutSeconds);
        assertEquals(180, config.requestTimeoutSeconds);
        assertTrue(config.protectExistingBlocks);
        assertEquals(5000, config.maxBlocksPerAction);
        assertEquals(8, config.scanRadius);
        assertEquals(5, config.scanForwardBlocks);
        assertEquals(8000, config.maxScanBlocks);
        assertTrue(config.pollControllerJobs);
        assertEquals(40, config.pollIntervalTicks);
        assertTrue(Files.exists(path));
    }

    @Test
    void normalizesConfigValuesFromDisk() throws Exception {
        Path path = Files.createTempDirectory("blockwright-fabric-config").resolve("blockwright.json");
        Files.writeString(path, """
                {
                  "controllerUrl": "http://127.0.0.1:8765/",
                  "serverId": "",
                  "sharedToken": null,
                  "connectTimeoutSeconds": 0,
                  "requestTimeoutSeconds": 5,
                  "maxBlocksPerAction": 100000,
                  "scanRadius": 1,
                  "scanForwardBlocks": 100,
                  "maxScanBlocks": 1,
                  "pollIntervalTicks": 1
                }
                """);

        BlockwrightConfig config = BlockwrightConfig.load(path);

        assertEquals("http://127.0.0.1:8765", config.controllerUrl);
        assertEquals("hmcl-lan", config.serverId);
        assertEquals("", config.sharedToken);
        assertEquals(1, config.connectTimeoutSeconds);
        assertEquals(30, config.requestTimeoutSeconds);
        assertEquals(50_000, config.maxBlocksPerAction);
        assertEquals(3, config.scanRadius);
        assertEquals(12, config.scanForwardBlocks);
        assertEquals(100, config.maxScanBlocks);
        assertEquals(5, config.pollIntervalTicks);
    }
}
