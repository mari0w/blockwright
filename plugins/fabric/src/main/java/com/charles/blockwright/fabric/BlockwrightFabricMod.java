package com.charles.blockwright.fabric;

import com.mojang.brigadier.arguments.StringArgumentType;
import java.nio.file.Path;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
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
    private static final ExecutorService REQUEST_EXECUTOR =
            Executors.newSingleThreadExecutor(runnable -> {
                Thread thread = new Thread(runnable, "blockwright-controller-client");
                thread.setDaemon(true);
                return thread;
            });

    private static BlockwrightConfig config;
    private static JobPoller jobPoller;

    @Override
    public void onInitialize() {
        reloadConfig();
        CommandRegistrationCallback.EVENT.register((dispatcher, registryAccess, environment) -> dispatcher.register(
                CommandManager.literal("bw")
                        .then(CommandManager.literal("reload").executes(context -> reload(context.getSource())))
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
        ServerTickEvents.END_SERVER_TICK.register(server -> {
            if (jobPoller != null) {
                jobPoller.tick();
            }
        });
        LOGGER.info("Blockwright Fabric mod initialized");
    }

    private static int reload(ServerCommandSource source) {
        reloadConfig();
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

        CompletableFuture
                .supplyAsync(() -> sendRequest(controllerClient, playerSnapshot, text, nearbyScan), REQUEST_EXECUTOR)
                .thenAccept(response -> server.execute(() -> {
                    ServerPlayerEntity currentPlayer = server.getPlayerManager().getPlayer(playerName);
                    if (currentPlayer == null) {
                        LOGGER.warn("player left before Blockwright response: {}", playerName);
                        return;
                    }
                    currentPlayer.sendMessage(Text.literal(response.reply), false);
                    executeDirectActions(controllerClient, server, currentPlayer, response);
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
        try {
            return controllerClient.sendMinecraftMessage(player, text, nearbyScan);
        } catch (Exception error) {
            throw new IllegalStateException(error);
        }
    }

    private static void executeDirectActions(
            ControllerClient controllerClient,
            MinecraftServer server,
            ServerPlayerEntity player,
            JsonModels.MinecraftMessageResponse response) {
        boolean ok = true;
        String message = "ok";
        JsonModels.JobExecutionReport report = null;

        try {
            report = new ActionExecutor(server, config).executeActions(response.actions, player);
            ok = report.isOk();
            if (!ok) {
                message = "建筑校验失败，已回传差异报告";
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
