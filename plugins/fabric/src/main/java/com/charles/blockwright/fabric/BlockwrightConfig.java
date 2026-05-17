package com.charles.blockwright.fabric;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

public final class BlockwrightConfig {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();

    public String controllerUrl = "http://127.0.0.1:8765";
    public String serverId = "hmcl-lan";
    public String sharedToken = "local-dev-token";
    public boolean protectExistingBlocks = true;
    public int maxBlocksPerAction = 5000;
    public int scanRadius = 8;
    public int scanForwardBlocks = 5;
    public int maxScanBlocks = 8000;
    public boolean pollControllerJobs = true;
    public int pollIntervalTicks = 40;

    public static BlockwrightConfig load(Path path) throws IOException {
        if (Files.notExists(path)) {
            BlockwrightConfig config = new BlockwrightConfig();
            save(path, config);
            return config;
        }

        String content = Files.readString(path, StandardCharsets.UTF_8);
        BlockwrightConfig config = GSON.fromJson(content, BlockwrightConfig.class);
        if (config == null) {
            config = new BlockwrightConfig();
        }
        config.normalize();
        return config;
    }

    public static void save(Path path, BlockwrightConfig config) throws IOException {
        Path parent = path.getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        config.normalize();
        Files.writeString(path, GSON.toJson(config), StandardCharsets.UTF_8);
    }

    private void normalize() {
        controllerUrl = ControllerPaths.trimTrailingSlash(controllerUrl);
        if (serverId == null || serverId.isBlank()) {
            serverId = "hmcl-lan";
        }
        if (sharedToken == null) {
            sharedToken = "";
        }
        maxBlocksPerAction = PlacementPolicy.normalizeMaxBlocks(maxBlocksPerAction);
        scanRadius = normalizeBounded(scanRadius, 3, 16);
        scanForwardBlocks = normalizeBounded(scanForwardBlocks, 0, 12);
        maxScanBlocks = normalizeBounded(maxScanBlocks, 100, 20000);
        pollIntervalTicks = normalizePollIntervalTicks(pollIntervalTicks);
    }

    private int normalizeBounded(int value, int min, int max) {
        if (value < min) {
            return min;
        }
        return Math.min(value, max);
    }

    private int normalizePollIntervalTicks(int value) {
        if (value < 5) {
            return 5;
        }
        return Math.min(value, 20 * 60);
    }
}
