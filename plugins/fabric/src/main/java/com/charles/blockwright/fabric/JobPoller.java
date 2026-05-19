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
            if (executeScanAndPlanJob(controllerClient, job, player)) {
                return;
            }
            report = new ActionExecutor(server, configSupplier.get()).executeActions(job.actions, player);
            ok = report.isOk();
            if (!ok) {
                message = "建筑校验失败，已回传差异报告";
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

    private boolean executeScanAndPlanJob(
            ControllerClient controllerClient,
            JsonModels.GameJob job,
            ServerPlayerEntity player) {
        JsonModels.GameAction scanAction = firstScanAction(job.actions);
        if (scanAction == null) {
            return false;
        }

        PlayerSnapshot playerSnapshot = PlayerSnapshot.from(player);
        JsonModels.WorldScan nearbyScan = WorldScanner.scan(player, configSupplier.get());
        String text = scanAction.text == null || scanAction.text.isBlank()
                ? scanAction.message
                : scanAction.text;
        if (text == null || text.isBlank()) {
            CompletableFuture.runAsync(
                    () -> sendJobResult(controllerClient, job.id, false, "缺少要继续处理的原始需求", null),
                    executor);
            return true;
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
                .thenAccept(response -> server.execute(() -> executeScannedResponse(controllerClient, job, playerSnapshot, response)))
                .exceptionally(error -> {
                    CompletableFuture.runAsync(
                            () -> sendJobResult(controllerClient, job.id, false, rootMessage(error), null),
                            executor);
                    LOGGER.warn("Blockwright queued scan planning failed", error);
                    return null;
                });
        return true;
    }

    private JsonModels.GameAction firstScanAction(List<JsonModels.GameAction> actions) {
        if (actions == null) {
            return null;
        }

        for (JsonModels.GameAction action : actions) {
            if (action != null && "scan_nearby_and_plan".equals(action.type)) {
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
            JsonModels.GameJob originalJob,
            PlayerSnapshot playerSnapshot,
            JsonModels.MinecraftMessageResponse response) {
        ServerPlayerEntity currentPlayer = server.getPlayerManager().getPlayer(playerSnapshot.name());
        if (currentPlayer == null) {
            CompletableFuture.runAsync(
                    () -> sendJobResult(controllerClient, originalJob.id, false, "玩家已离线", null),
                    executor);
            return;
        }

        currentPlayer.sendMessage(net.minecraft.text.Text.literal(response.reply), false);
        boolean ok = true;
        String message = "ok";
        JsonModels.JobExecutionReport report = null;
        try {
            report = new ActionExecutor(server, configSupplier.get()).executeActions(response.actions, currentPlayer);
            ok = report.isOk();
            if (!ok) {
                message = "建筑校验失败，已回传差异报告";
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
                    sendJobResult(controllerClient, originalJob.id, resultOk, resultMessage, resultReport);
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
        try {
            controllerClient.sendJobResult(jobId, ok, message, report);
        } catch (Exception error) {
            LOGGER.warn("Blockwright send job result failed: {}", error.getMessage());
        }
    }
}
