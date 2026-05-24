package com.charles.blockwright.fabric;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

public final class BlockwrightConfig {
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private static final int MAX_REQUEST_TIMEOUT_SECONDS = 30 * 60;

    public String controllerUrl = "http://127.0.0.1:8765";
    public boolean autoStartController = true;
    public String controllerLaunchCommand = "";
    public String controllerWorkingDirectory = "";
    public int controllerStartupTimeoutSeconds = 120;
    public String serverId = "hmcl-lan";
    public String sharedToken = "local-dev-token";
    public int connectTimeoutSeconds = 5;
    public int requestTimeoutSeconds = MAX_REQUEST_TIMEOUT_SECONDS;
    public boolean protectExistingBlocks = true;
    public int maxBlocksPerAction = 0;
    public int scanRadius = 8;
    public int scanForwardBlocks = 5;
    public int maxScanBlocks = 8000;
    public boolean pollControllerJobs = true;
    public int pollIntervalTicks = 40;
    public boolean matrixEnabled = true;
    public String matrixHomeserverUrl = "https://matrix-client.matrix.org";
    public String matrixAccessToken = "";
    public String matrixAllowedSender = "@enochzzg:matrix.org";
    public String matrixDefaultTargetPlayer = "Charles";
    public boolean matrixAllowOwnUserMessages = true;
    public boolean matrixAutoJoinInvites = true;

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
        save(path, config);
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
        if (controllerLaunchCommand == null) {
            controllerLaunchCommand = "";
        } else {
            controllerLaunchCommand = controllerLaunchCommand.trim();
        }
        if (controllerWorkingDirectory == null) {
            controllerWorkingDirectory = "";
        } else {
            controllerWorkingDirectory = controllerWorkingDirectory.trim();
        }
        controllerStartupTimeoutSeconds = normalizeBounded(controllerStartupTimeoutSeconds, 5, 600);
        if (serverId == null || serverId.isBlank()) {
            serverId = "hmcl-lan";
        }
        if (sharedToken == null) {
            sharedToken = "";
        }
        connectTimeoutSeconds = normalizeBounded(connectTimeoutSeconds, 1, 30);
        requestTimeoutSeconds = normalizeRequestTimeout(requestTimeoutSeconds);
        maxBlocksPerAction = PlacementPolicy.normalizeMaxBlocks(maxBlocksPerAction);
        scanRadius = normalizeBounded(scanRadius, 3, 16);
        scanForwardBlocks = normalizeBounded(scanForwardBlocks, 0, 12);
        maxScanBlocks = normalizeBounded(maxScanBlocks, 100, 20000);
        pollIntervalTicks = normalizePollIntervalTicks(pollIntervalTicks);
        if (matrixHomeserverUrl == null || matrixHomeserverUrl.isBlank()) {
            matrixHomeserverUrl = "https://matrix-client.matrix.org";
        } else {
            matrixHomeserverUrl = ControllerPaths.trimTrailingSlash(matrixHomeserverUrl);
        }
        if (matrixAccessToken == null) {
            matrixAccessToken = "";
        }
        if (matrixAllowedSender == null || matrixAllowedSender.isBlank()) {
            matrixAllowedSender = "@enochzzg:matrix.org";
        } else {
            matrixAllowedSender = matrixAllowedSender.trim();
        }
        if (matrixDefaultTargetPlayer == null || matrixDefaultTargetPlayer.isBlank()) {
            matrixDefaultTargetPlayer = "Charles";
        } else {
            matrixDefaultTargetPlayer = matrixDefaultTargetPlayer.trim();
        }
    }

    private int normalizeBounded(int value, int min, int max) {
        if (value < min) {
            return min;
        }
        return Math.min(value, max);
    }

    private int normalizeRequestTimeout(int value) {
        if (value != MAX_REQUEST_TIMEOUT_SECONDS) {
            return MAX_REQUEST_TIMEOUT_SECONDS;
        }
        return value;
    }

    private int normalizePollIntervalTicks(int value) {
        if (value < 5) {
            return 5;
        }
        return Math.min(value, 20 * 60);
    }
}
