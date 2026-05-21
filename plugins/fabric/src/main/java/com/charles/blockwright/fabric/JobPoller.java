package com.charles.blockwright.fabric;

import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.Supplier;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.network.ServerPlayerEntity;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public final class JobPoller {
    private static final Logger LOGGER = LoggerFactory.getLogger(BlockwrightFabricMod.MOD_ID);
    private static final int MAX_SCAN_REPLAN_ATTEMPTS = 3;

    private final MinecraftServer server;
    private final Supplier<BlockwrightConfig> configSupplier;
    private final ExecutorService executor;
    private final AtomicBoolean polling = new AtomicBoolean(false);
    private int ticks;

    public JobPoller(
            MinecraftServer server,
            Supplier<BlockwrightConfig> configSupplier,
            ExecutorService executor) {
        this.server = server;
        this.configSupplier = configSupplier;
        this.executor = executor;
    }

    public void tick() {
        BlockwrightConfig config = configSupplier.get();
        if (!config.pollControllerJobs || server.getPlayerManager().getPlayerList().isEmpty()) {
            return;
        }

        ticks++;
        if (ticks < config.pollIntervalTicks) {
            return;
        }
        ticks = 0;

        if (!polling.compareAndSet(false, true)) {
            return;
        }

        ControllerClient controllerClient = new ControllerClient(config);
        CompletableFuture
                .supplyAsync(() -> requestNextJob(controllerClient), executor)
                .whenComplete((response, error) -> {
                    if (error != null) {
                        polling.set(false);
                        LOGGER.debug("Blockwright job poll failed", error);
                        return;
                    }

                    JsonModels.GameJob job = response == null ? null : response.job;
                    if (job == null) {
                        polling.set(false);
                        return;
                    }

                    server.execute(() -> {
                        try {
                            executeJob(controllerClient, job);
                        } finally {
                            polling.set(false);
                        }
                    });
                });
    }

    private JsonModels.NextJobResponse requestNextJob(ControllerClient controllerClient) {
        try {
            return controllerClient.nextJob();
        } catch (Exception error) {
            throw new IllegalStateException(error);
        }
    }

    private void executeJob(ControllerClient controllerClient, JsonModels.GameJob job) {
        boolean ok = true;
        String message = "ok";
        JsonModels.JobExecutionReport report = null;

        try {
            ServerPlayerEntity player = resolveTargetPlayer(job.targetPlayer);
            if (player == null) {
                throw new IllegalStateException("没有在线玩家可执行任务");
            }
            if (executeLiveQueryJob(controllerClient, job, player)) {
                return;
            }
            if (executeScanAndPlanJob(controllerClient, job, player, 0)) {
                return;
            }
            report = new ActionExecutor(server, configSupplier.get()).executeActions(job.actions, player);
            ok = report.isOk();
            if (!ok) {
                message = "建筑执行失败，已回传执行报告";
            }
        } catch (Exception error) {
            ok = false;
            message = error.getMessage();
            LOGGER.warn("Blockwright job execute failed: {}, {}", job.id, error.getMessage());
        }

        boolean resultOk = ok;
        String resultMessage = message;
        JsonModels.JobExecutionReport resultReport = report;
        CompletableFuture.runAsync(
                () -> sendJobResult(controllerClient, job.id, resultOk, resultMessage, resultReport),
                executor);
    }

    private boolean executeLiveQueryJob(
            ControllerClient controllerClient,
            JsonModels.GameJob job,
            ServerPlayerEntity player) {
        JsonModels.GameAction stateAction = firstAction(job.actions, "get_player_state");
        if (stateAction != null) {
            PlayerSnapshot snapshot = PlayerSnapshot.from(player);
            JsonModels.JobResultRequest result = new JsonModels.JobResultRequest();
            result.ok = true;
            result.message = "ok";
            result.playerState = snapshot.playerState();
            CompletableFuture.runAsync(
                    () -> sendJobResult(controllerClient, job.id, result),
                    executor);
            return true;
        }

        JsonModels.GameAction scanAction = firstAction(job.actions, "scan_nearby");
        if (scanAction != null) {
            JsonModels.JobResultRequest result = new JsonModels.JobResultRequest();
            result.ok = true;
            result.message = "ok";
            result.nearbyScan = WorldScanner.scan(player, configSupplier.get(), scanAction.radius);
            CompletableFuture.runAsync(
                    () -> sendJobResult(controllerClient, job.id, result),
                    executor);
            return true;
        }

        return false;
    }

    private boolean executeScanAndPlanJob(
            ControllerClient controllerClient,
            JsonModels.GameJob job,
            ServerPlayerEntity player,
            int attempt) {
        JsonModels.GameAction scanAction = firstScanAction(job.actions);
        if (scanAction == null) {
            return false;
        }

        executeScanAndPlanAction(controllerClient, job.id, player, scanAction, attempt);
        return true;
    }

    private void executeScanAndPlanAction(
            ControllerClient controllerClient,
            String originalJobId,
            ServerPlayerEntity player,
            JsonModels.GameAction scanAction,
            int attempt) {
        if (attempt >= MAX_SCAN_REPLAN_ATTEMPTS) {
            CompletableFuture.runAsync(
                    () -> sendJobResult(
                            controllerClient,
                            originalJobId,
                            false,
                            "连续扫描后仍未生成可执行方案，已停止，避免一直重复扫描。",
                            null),
                    executor);
            return;
        }

        PlayerSnapshot playerSnapshot = PlayerSnapshot.from(player);
        JsonModels.WorldScan nearbyScan = WorldScanner.scan(player, configSupplier.get());
        String text = scanAction.text == null || scanAction.text.isBlank()
                ? scanAction.message
                : scanAction.text;
        if (text == null || text.isBlank()) {
            CompletableFuture.runAsync(
                    () -> sendJobResult(controllerClient, originalJobId, false, "缺少要继续处理的原始需求", null),
                    executor);
            return;
        }

        CompletableFuture
                .supplyAsync(
                        () -> sendScannedRequest(
                                controllerClient,
                                playerSnapshot,
                                text,
                                scanAction.attachments,
                                nearbyScan),
                        executor)
                .thenAccept(response -> server.execute(() -> executeScannedResponse(
                        controllerClient,
                        originalJobId,
                        playerSnapshot,
                        response,
                        attempt + 1)))
                .exceptionally(error -> {
                    CompletableFuture.runAsync(
                            () -> sendJobResult(controllerClient, originalJobId, false, rootMessage(error), null),
                            executor);
                    LOGGER.warn("Blockwright queued scan planning failed", error);
                    return null;
                });
    }

    private JsonModels.GameAction firstScanAction(List<JsonModels.GameAction> actions) {
        return firstAction(actions, "scan_nearby_and_plan");
    }

    private JsonModels.GameAction firstAction(List<JsonModels.GameAction> actions, String type) {
        if (actions == null) {
            return null;
        }

        for (JsonModels.GameAction action : actions) {
            if (action != null && type.equals(action.type)) {
                return action;
            }
        }
        return null;
    }

    private JsonModels.MinecraftMessageResponse sendScannedRequest(
            ControllerClient controllerClient,
            PlayerSnapshot playerSnapshot,
            String text,
            List<JsonModels.ChatAttachment> attachments,
            JsonModels.WorldScan nearbyScan) {
        try {
            return controllerClient.sendMinecraftMessage(
                    playerSnapshot,
                    text,
                    nearbyScan,
                    attachments,
                    progress -> {
                        if (progress != null && progress.message != null && !progress.message.isBlank()) {
                            LOGGER.info(
                                    "Blockwright Codex progress [queued-scan:{} #{}]: {}",
                                    playerSnapshot.name(),
                                    progress.sequence,
                                    progress.message);
                        }
                    });
        } catch (Exception error) {
            throw new IllegalStateException(error);
        }
    }

    private void executeScannedResponse(
            ControllerClient controllerClient,
            String originalJobId,
            PlayerSnapshot playerSnapshot,
            JsonModels.MinecraftMessageResponse response,
            int attempt) {
        ServerPlayerEntity currentPlayer = server.getPlayerManager().getPlayer(playerSnapshot.name());
        if (currentPlayer == null) {
            CompletableFuture.runAsync(
                    () -> sendJobResult(controllerClient, originalJobId, false, "玩家已离线", null),
                    executor);
            return;
        }

        currentPlayer.sendMessage(net.minecraft.text.Text.literal(response.reply), false);
        JsonModels.GameAction nextScanAction = firstScanAction(response.actions);
        if (nextScanAction != null) {
            executeScanAndPlanAction(controllerClient, originalJobId, currentPlayer, nextScanAction, attempt);
            return;
        }

        boolean ok = true;
        String message = "ok";
        JsonModels.JobExecutionReport report = null;
        try {
            report = new ActionExecutor(server, configSupplier.get()).executeActions(response.actions, currentPlayer);
            ok = report.isOk();
            if (!ok) {
                message = "建筑执行失败，已回传执行报告";
            }
        } catch (Exception error) {
            ok = false;
            message = rootMessage(error);
            LOGGER.warn("Blockwright queued scanned action failed", error);
        }

        boolean resultOk = ok;
        String resultMessage = message;
        JsonModels.JobExecutionReport resultReport = report;
        CompletableFuture.runAsync(
                () -> {
                    sendJobResult(controllerClient, originalJobId, resultOk, resultMessage, resultReport);
                    if (response.jobId != null && !response.jobId.isBlank()) {
                        sendJobResult(controllerClient, response.jobId, resultOk, resultMessage, resultReport);
                    }
                },
                executor);
    }

    private String rootMessage(Throwable error) {
        Throwable current = error;
        while (current.getCause() != null) {
            current = current.getCause();
        }
        return current.getMessage() == null ? current.toString() : current.getMessage();
    }

    private ServerPlayerEntity resolveTargetPlayer(String targetPlayer) {
        if (targetPlayer != null && !targetPlayer.isBlank()) {
            return server.getPlayerManager().getPlayer(targetPlayer);
        }

        List<ServerPlayerEntity> players = server.getPlayerManager().getPlayerList();
        return players.isEmpty() ? null : players.get(0);
    }

    private void sendJobResult(
            ControllerClient controllerClient,
            String jobId,
            boolean ok,
            String message,
            JsonModels.JobExecutionReport report) {
        JsonModels.JobResultRequest request = new JsonModels.JobResultRequest();
        request.ok = ok;
        request.message = message;
        request.report = report;
        sendJobResult(controllerClient, jobId, request);
    }

    private void sendJobResult(
            ControllerClient controllerClient,
            String jobId,
            JsonModels.JobResultRequest request) {
        try {
            controllerClient.sendJobResult(jobId, request);
        } catch (Exception error) {
            LOGGER.warn("Blockwright send job result failed: {}", error.getMessage());
        }
    }
}
