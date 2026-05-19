package com.charles.blockwright;

import org.bukkit.Location;
import org.bukkit.entity.Player;
import org.bukkit.scheduler.BukkitTask;

public final class JobPoller {
    private final BlockwrightPlugin plugin;
    private final ControllerClient controllerClient;
    private final ActionExecutor actionExecutor;
    private BukkitTask task;

    public JobPoller(BlockwrightPlugin plugin, ControllerClient controllerClient, ActionExecutor actionExecutor) {
        this.plugin = plugin;
        this.controllerClient = controllerClient;
        this.actionExecutor = actionExecutor;
    }

    public void start() {
        long interval = plugin.getConfig().getLong("poll-interval-ticks", 40L);
        task = plugin.getServer().getScheduler().runTaskTimerAsynchronously(plugin, this::pollOnce, interval, interval);
    }

    public void cancel() {
        if (task != null) {
            task.cancel();
            task = null;
        }
    }

    private void pollOnce() {
        try {
            JsonModels.NextJobResponse response = controllerClient.nextJob();
            if (response == null || response.job == null) {
                return;
            }

            JsonModels.GameJob job = response.job;
            plugin.getServer().getScheduler().runTask(plugin, () -> executeJob(job));
        } catch (Exception error) {
            plugin.getLogger().fine("job poll failed: " + error.getMessage());
        }
    }

    private void executeJob(JsonModels.GameJob job) {
        boolean ok = true;
        String message = "ok";
        JsonModels.JobExecutionReport report = null;

        try {
            if (executeLiveQueryJob(job)) {
                return;
            }
            Location origin = defaultOrigin(job.targetPlayer);
            report = actionExecutor.executeActions(job.actions, job.targetPlayer, origin);
            ok = report.isOk();
            if (!ok) {
                message = "建筑校验失败，已回传差异报告";
            }
        } catch (Exception error) {
            ok = false;
            message = error.getMessage();
            plugin.getLogger().warning("job execute failed: " + job.id + ", " + error.getMessage());
        }

        boolean resultOk = ok;
        String resultMessage = message;
        JsonModels.JobExecutionReport resultReport = report;
        plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
            try {
                controllerClient.sendJobResult(job.id, resultOk, resultMessage, resultReport);
            } catch (Exception error) {
                plugin.getLogger().warning("send job result failed: " + error.getMessage());
            }
        });
    }

    private boolean executeLiveQueryJob(JsonModels.GameJob job) {
        JsonModels.GameAction stateAction = firstAction(job.actions, "get_player_state");
        if (stateAction != null) {
            Player player = resolveTargetPlayer(job.targetPlayer);
            JsonModels.JobResultRequest result = new JsonModels.JobResultRequest();
            result.ok = player != null;
            result.message = player == null ? "没有在线玩家可执行查询" : "ok";
            if (player != null) {
                result.playerState = JsonModels.PlayerState.fromPlayer(player);
            }
            sendJobResultAsync(job.id, result);
            return true;
        }

        JsonModels.GameAction scanAction = firstAction(job.actions, "scan_nearby");
        if (scanAction != null) {
            Player player = resolveTargetPlayer(job.targetPlayer);
            JsonModels.JobResultRequest result = new JsonModels.JobResultRequest();
            result.ok = player != null;
            result.message = player == null ? "没有在线玩家可执行扫描" : "ok";
            if (player != null) {
                result.nearbyScan = WorldScanner.scan(player, scanAction.radius);
            }
            sendJobResultAsync(job.id, result);
            return true;
        }

        return false;
    }

    private JsonModels.GameAction firstAction(java.util.List<JsonModels.GameAction> actions, String type) {
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

    private Player resolveTargetPlayer(String targetPlayer) {
        if (targetPlayer != null && !targetPlayer.isBlank()) {
            return plugin.getServer().getPlayerExact(targetPlayer);
        }
        return plugin.getServer().getOnlinePlayers().stream().findFirst().orElse(null);
    }

    private void sendJobResultAsync(String jobId, JsonModels.JobResultRequest result) {
        plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
            try {
                controllerClient.sendJobResult(jobId, result);
            } catch (Exception error) {
                plugin.getLogger().warning("send job result failed: " + error.getMessage());
            }
        });
    }

    private Location defaultOrigin(String targetPlayer) {
        if (targetPlayer != null) {
            Player player = plugin.getServer().getPlayerExact(targetPlayer);
            if (player != null) {
                return player.getLocation();
            }
        }
        return plugin.getServer().getWorlds().get(0).getSpawnLocation();
    }
}
