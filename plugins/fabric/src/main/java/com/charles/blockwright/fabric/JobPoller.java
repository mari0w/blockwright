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
