package com.charles.blockwright.fabric;

import com.mojang.brigadier.arguments.StringArgumentType;
import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.fabric.api.networking.v1.ServerPlayConnectionEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.command.CommandManager;
import net.minecraft.server.command.ServerCommandSource;
import net.minecraft.server.network.ServerPlayerEntity;
import net.minecraft.text.Text;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public final class BlockwrightFabricMod implements ModInitializer {
    public static final String MOD_ID = "blockwright";
    private static final Logger LOGGER = LoggerFactory.getLogger(MOD_ID);
    private static final int MAX_SCAN_REPLAN_ATTEMPTS = 3;
    private static final ExecutorService REQUEST_EXECUTOR =
            Executors.newSingleThreadExecutor(runnable -> {
                Thread thread = new Thread(runnable, "blockwright-controller-client");
                thread.setDaemon(true);
                return thread;
            });

    private static BlockwrightConfig config;
    private static JobPoller jobPoller;
    private static Path gameDir;

    @Override
    public void onInitialize() {
        gameDir = FabricLoader.getInstance().getGameDir();
        reloadConfig();
        ControllerProcessManager.ensureStartedAsync(config, gameDir);
        Runtime.getRuntime().addShutdownHook(new Thread(
                ControllerProcessManager::stopIfLaunched,
                "blockwright-controller-shutdown"));
        CommandRegistrationCallback.EVENT.register((dispatcher, registryAccess, environment) -> dispatcher.register(
                CommandManager.literal("bw")
                        .then(CommandManager.literal("reload").executes(context -> reload(context.getSource())))
                        .then(CommandManager.literal("config").executes(context -> configHint(context.getSource())))
                        .then(CommandManager.literal("web").executes(context -> webAddress(context.getSource())))
                        .then(CommandManager.literal("url").executes(context -> webAddress(context.getSource())))
                        .then(CommandManager.literal("address").executes(context -> webAddress(context.getSource())))
                        .then(CommandManager.literal("lan").executes(context -> webAddress(context.getSource())))
                        .then(CommandManager.literal("ask")
                                .then(CommandManager.argument("message", StringArgumentType.greedyString())
                                        .executes(context -> runChat(
                                                context.getSource(),
                                                StringArgumentType.getString(context, "message")))))
                        .then(CommandManager.literal("chat")
                                .then(CommandManager.argument("message", StringArgumentType.greedyString())
                                        .executes(context -> runChat(
                                                context.getSource(),
                                                StringArgumentType.getString(context, "message")))))
                        .then(CommandManager.argument("message", StringArgumentType.greedyString())
                                .executes(context -> runChat(
                                        context.getSource(),
                                        StringArgumentType.getString(context, "message"))))));
        ServerLifecycleEvents.SERVER_STARTED.register(server -> jobPoller =
                new JobPoller(server, () -> config, REQUEST_EXECUTOR));
        ServerPlayConnectionEvents.JOIN.register((handler, sender, server) -> {
            ControllerProcessManager.ensureStartedAsync(config, gameDir);
            sendStartupHint(handler.getPlayer());
        });
        ServerTickEvents.END_SERVER_TICK.register(server -> {
            if (jobPoller != null) {
                jobPoller.tick();
            }
        });
        LOGGER.info("Blockwright Fabric mod initialized");
    }

    private static void sendStartupHint(ServerPlayerEntity player) {
        for (String message : ControllerProcessManager.startupHintMessages(config)) {
            player.sendMessage(Text.literal(message), false);
        }
    }

    private static int configHint(ServerCommandSource source) {
        String message = config.autoStartController
                ? "Blockwright Web 会随模组自动启动，请打开 " + config.controllerUrl + "/web 配置；也可以用 /bw web 查看局域网地址。"
                : "Blockwright Web 自动启动已关闭，请先手动启动 controller，再打开 "
                        + config.controllerUrl
                        + "/web 配置；也可以用 /bw web 查看局域网地址。";
        source.sendFeedback(
                () -> Text.literal(message),
                false);
        return 1;
    }

    private static int webAddress(ServerCommandSource source) {
        ControllerProcessManager.ensureStartedAsync(config, gameDir);
        for (String message : ControllerProcessManager.webAddressMessages(config.controllerUrl)) {
            String line = message;
            source.sendFeedback(() -> Text.literal(line), false);
        }
        return 1;
    }

    private static int reload(ServerCommandSource source) {
        reloadConfig();
        ControllerProcessManager.ensureStartedAsync(config, gameDir);
        source.sendFeedback(() -> Text.literal("Blockwright 配置已重新加载。"), false);
        return 1;
    }

    private static int runChat(ServerCommandSource source, String text) {
        ServerPlayerEntity player = source.getPlayer();
        if (player == null) {
            source.sendError(Text.literal("这个命令需要玩家在游戏内执行。"));
            return 0;
        }

        MinecraftServer server = source.getServer();
        PlayerSnapshot playerSnapshot = PlayerSnapshot.from(player);
        JsonModels.WorldScan nearbyScan = WorldScanner.scan(player, config);
        String playerName = playerSnapshot.name();
        player.sendMessage(Text.literal("Blockwright 正在处理你的需求..."), false);
        ControllerClient controllerClient = new ControllerClient(config);
        ControllerClient.ProgressListener progressListener = progressLogger("direct", playerName);

        CompletableFuture
                .supplyAsync(
                        () -> sendRequest(
                                controllerClient,
                                playerSnapshot,
                                text,
                                nearbyScan,
                                null,
                                progressListener),
                        REQUEST_EXECUTOR)
                .thenAccept(response -> server.execute(() -> {
                    ServerPlayerEntity currentPlayer = server.getPlayerManager().getPlayer(playerName);
                    if (currentPlayer == null) {
                        LOGGER.warn("player left before Blockwright response: {}", playerName);
                        return;
                    }
                    currentPlayer.sendMessage(Text.literal(response.reply), false);
                    if (!executeScanAndPlanAction(controllerClient, server, currentPlayer, response, 0)) {
                        executeDirectActions(controllerClient, server, currentPlayer, response);
                    }
                }))
                .exceptionally(error -> {
                    server.execute(() -> {
                        ServerPlayerEntity currentPlayer = server.getPlayerManager().getPlayer(playerName);
                        if (currentPlayer != null) {
                            currentPlayer.sendMessage(
                                    Text.literal("Blockwright controller 请求失败：" + rootMessage(error)),
                                    false);
                        }
                    });
                    LOGGER.warn("controller request failed", error);
                    return null;
                });
        return 1;
    }

    private static JsonModels.MinecraftMessageResponse sendRequest(
            ControllerClient controllerClient,
            PlayerSnapshot player,
            String text,
            JsonModels.WorldScan nearbyScan) {
        return sendRequest(controllerClient, player, text, nearbyScan, null);
    }

    private static JsonModels.MinecraftMessageResponse sendRequest(
            ControllerClient controllerClient,
            PlayerSnapshot player,
            String text,
            JsonModels.WorldScan nearbyScan,
            List<JsonModels.ChatAttachment> attachments) {
        return sendRequest(controllerClient, player, text, nearbyScan, attachments, null);
    }

    private static JsonModels.MinecraftMessageResponse sendRequest(
            ControllerClient controllerClient,
            PlayerSnapshot player,
            String text,
            JsonModels.WorldScan nearbyScan,
            List<JsonModels.ChatAttachment> attachments,
            ControllerClient.ProgressListener progressListener) {
        try {
            return controllerClient.sendMinecraftMessage(player, text, nearbyScan, attachments, progressListener);
        } catch (Exception error) {
            throw new IllegalStateException(error);
        }
    }

    private static ControllerClient.ProgressListener progressLogger(String scope, String playerName) {
        return progress -> {
            if (progress == null || progress.message == null || progress.message.isBlank()) {
                return;
            }
            LOGGER.info("Blockwright Codex progress [{}:{} #{}]: {}",
                    scope,
                    playerName,
                    progress.sequence,
                    progress.message);
        };
    }

    private static void executeDirectActions(
            ControllerClient controllerClient,
            MinecraftServer server,
            ServerPlayerEntity player,
            JsonModels.MinecraftMessageResponse response) {
        if (JobPoller.hasPlaceBlocks(response.actions)) {
            if (jobPoller != null
                    && jobPoller.startControlledActions(
                            controllerClient,
                            response.jobId,
                            player.getName().getString(),
                            "直接执行玩家请求",
                            response.actions,
                            player,
                            null)) {
                return;
            }
            player.sendMessage(Text.literal("Blockwright 正在执行另一个建筑任务，请等它完成后再试。"), false);
            if (response.jobId != null && !response.jobId.isBlank()) {
                CompletableFuture.runAsync(
                        () -> sendDirectJobResult(
                                controllerClient,
                                response.jobId,
                                false,
                                "执行端正忙，建筑任务未开始。",
                                null),
                        REQUEST_EXECUTOR);
            }
            return;
        }

        boolean ok = true;
        String message = "ok";
        JsonModels.JobExecutionReport report = null;

        try {
            report = new ActionExecutor(server, config).executeActions(response.actions, player);
            ok = report.isOk();
            if (!ok) {
                message = "建筑执行失败，已回传执行报告";
                player.sendMessage(Text.literal(message), false);
            }
        } catch (Exception error) {
            ok = false;
            message = rootMessage(error);
            player.sendMessage(Text.literal("Blockwright 执行失败：" + message), false);
            LOGGER.warn("Blockwright direct action failed", error);
        }

        if (response.jobId != null && !response.jobId.isBlank()) {
            boolean resultOk = ok;
            String resultMessage = message;
            JsonModels.JobExecutionReport resultReport = report;
            CompletableFuture.runAsync(
                    () -> sendDirectJobResult(controllerClient, response.jobId, resultOk, resultMessage, resultReport),
                    REQUEST_EXECUTOR);
        }
    }

    private static boolean executeScanAndPlanAction(
            ControllerClient controllerClient,
            MinecraftServer server,
            ServerPlayerEntity player,
            JsonModels.MinecraftMessageResponse response,
            int attempt) {
        if (response.actions == null) {
            return false;
        }

        for (JsonModels.GameAction action : response.actions) {
            if (action == null || !"scan_nearby_and_plan".equals(action.type)) {
                continue;
            }

            sendScannedRetry(controllerClient, server, player, action, attempt);
            return true;
        }

        return false;
    }

    private static void sendScannedRetry(
            ControllerClient controllerClient,
            MinecraftServer server,
            ServerPlayerEntity player,
            JsonModels.GameAction action,
            int attempt) {
        if (attempt >= MAX_SCAN_REPLAN_ATTEMPTS) {
            player.sendMessage(Text.literal("Blockwright 连续扫描后仍未生成可执行方案，已停止，避免一直重复扫描。"), false);
            return;
        }

        PlayerSnapshot playerSnapshot = PlayerSnapshot.from(player);
        JsonModels.WorldScan nearbyScan = WorldScanner.scan(player, config);
        String playerName = playerSnapshot.name();
        String text = action.text == null || action.text.isBlank() ? action.message : action.text;
        if (text == null || text.isBlank()) {
            player.sendMessage(Text.literal("Blockwright 扫描完成，但缺少要继续处理的原始需求。"), false);
            return;
        }

        CompletableFuture
                .supplyAsync(
                        () -> sendRequest(
                                controllerClient,
                                playerSnapshot,
                                text,
                                nearbyScan,
                                action.attachments,
                                progressLogger("scan-retry", playerName)),
                        REQUEST_EXECUTOR)
                .thenAccept(response -> server.execute(() -> {
                    ServerPlayerEntity currentPlayer = server.getPlayerManager().getPlayer(playerName);
                    if (currentPlayer == null) {
                        LOGGER.warn("player left before Blockwright scanned retry response: {}", playerName);
                        return;
                    }
                    currentPlayer.sendMessage(Text.literal(response.reply), false);
                    if (!executeScanAndPlanAction(controllerClient, server, currentPlayer, response, attempt + 1)) {
                        executeDirectActions(controllerClient, server, currentPlayer, response);
                    }
                }))
                .exceptionally(error -> {
                    server.execute(() -> {
                        ServerPlayerEntity currentPlayer = server.getPlayerManager().getPlayer(playerName);
                        if (currentPlayer != null) {
                            currentPlayer.sendMessage(
                                    Text.literal("Blockwright 扫描后继续处理失败：" + rootMessage(error)),
                                    false);
                        }
                    });
                    LOGGER.warn("Blockwright scanned retry failed", error);
                    return null;
                });
    }

    private static void sendDirectJobResult(
            ControllerClient controllerClient,
            String jobId,
            boolean ok,
            String message,
            JsonModels.JobExecutionReport report) {
        try {
            controllerClient.sendJobResult(jobId, ok, message, report);
        } catch (Exception error) {
            LOGGER.warn("Blockwright direct result report failed: {}", error.getMessage());
        }
    }

    private static void reloadConfig() {
        Path path = FabricLoader.getInstance().getConfigDir().resolve("blockwright.json");
        try {
            config = BlockwrightConfig.load(path);
        } catch (Exception error) {
            LOGGER.warn("failed to load Blockwright config, using defaults", error);
            config = new BlockwrightConfig();
        }
    }

    private static String rootMessage(Throwable error) {
        Throwable current = error;
        while (current.getCause() != null) {
            current = current.getCause();
        }
        return current.getMessage();
    }
}
