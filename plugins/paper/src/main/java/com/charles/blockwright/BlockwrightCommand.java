package com.charles.blockwright;

import org.bukkit.Location;
import org.bukkit.command.Command;
import org.bukkit.command.CommandExecutor;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;

public final class BlockwrightCommand implements CommandExecutor {
    private final BlockwrightPlugin plugin;
    private final ControllerClient controllerClient;
    private final ActionExecutor actionExecutor;

    public BlockwrightCommand(
            BlockwrightPlugin plugin,
            ControllerClient controllerClient,
            ActionExecutor actionExecutor) {
        this.plugin = plugin;
        this.controllerClient = controllerClient;
        this.actionExecutor = actionExecutor;
    }

    @Override
    public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        if (BlockwrightCommandText.isReload(args)) {
            plugin.reloadBlockwrightConfig();
            sender.sendMessage("Blockwright 配置已重新加载。");
            return true;
        }

        String text = BlockwrightCommandText.extractChatText(args);
        if (text == null || text.isBlank()) {
            sender.sendMessage(BlockwrightCommandText.usage());
            return true;
        }

        if (!(sender instanceof Player player)) {
            sender.sendMessage("这个命令需要玩家在游戏内执行。");
            return true;
        }

        sender.sendMessage("Blockwright 正在处理你的需求...");

        plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
            try {
                JsonModels.MinecraftMessageResponse response =
                        controllerClient.sendMinecraftMessage(
                                player,
                                text,
                                progress -> {
                                    if (progress != null
                                            && progress.message != null
                                            && !progress.message.isBlank()) {
                                        plugin.getLogger().info(String.format(
                                                "Blockwright Codex progress [direct:%s #%d]: %s",
                                                player.getName(),
                                                progress.sequence,
                                                progress.message));
                                    }
                                });
                plugin.getServer().getScheduler().runTask(plugin, () -> {
                    player.sendMessage(response.reply);
                    Location origin = player.getLocation();
                    executeActionsAndReport(response, player.getName(), origin);
                });
            } catch (Exception error) {
                plugin.getLogger().warning("controller request failed: " + error.getMessage());
                plugin.getServer().getScheduler().runTask(plugin,
                        () -> sender.sendMessage("Blockwright controller 请求失败：" + error.getMessage()));
            }
        });

        return true;
    }

    private void executeActionsAndReport(
            JsonModels.MinecraftMessageResponse response,
            String playerName,
            Location origin) {
        if (JobPoller.hasPlaceBlocks(response.actions)) {
            JobPoller poller = plugin.jobPoller();
            if (poller != null
                    && poller.startControlledActions(
                            response.jobId,
                            playerName,
                            "直接执行玩家请求",
                            response.actions,
                            origin)) {
                return;
            }
            sendPlayerMessage(playerName, "Blockwright 正在执行另一个建筑任务，请等它完成后再试。");
            if (response.jobId != null && !response.jobId.isBlank()) {
                plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
                    try {
                        controllerClient.sendJobResult(response.jobId, false, "执行端正忙，建筑任务未开始。", null);
                    } catch (Exception error) {
                        plugin.getLogger().warning("send direct job result failed: " + error.getMessage());
                    }
                });
            }
            return;
        }

        boolean ok = true;
        String message = "ok";
        JsonModels.JobExecutionReport report = null;

        try {
            report = actionExecutor.executeActions(response.actions, playerName, origin);
            ok = report.isOk();
            if (!ok) {
                message = "建筑执行失败，已回传执行报告";
                sendPlayerMessage(playerName, message);
            }
        } catch (Exception error) {
            ok = false;
            message = error.getMessage();
            plugin.getLogger().warning("action execute failed: " + error.getMessage());
            sendPlayerMessage(playerName, "Blockwright 执行失败：" + message);
        }

        if (response.jobId == null || response.jobId.isBlank()) {
            return;
        }

        boolean resultOk = ok;
        String resultMessage = message;
        JsonModels.JobExecutionReport resultReport = report;
        plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
            try {
                controllerClient.sendJobResult(response.jobId, resultOk, resultMessage, resultReport);
            } catch (Exception error) {
                plugin.getLogger().warning("send direct job result failed: " + error.getMessage());
            }
        });
    }

    private void sendPlayerMessage(String playerName, String message) {
        Player player = plugin.getServer().getPlayerExact(playerName);
        if (player != null) {
            player.sendMessage(message);
        }
    }
}
