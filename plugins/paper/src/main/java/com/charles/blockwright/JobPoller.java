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

        try {
            Location origin = defaultOrigin(job.targetPlayer);
            actionExecutor.executeActions(job.actions, job.targetPlayer, origin);
        } catch (Exception error) {
            ok = false;
            message = error.getMessage();
            plugin.getLogger().warning("job execute failed: " + job.id + ", " + error.getMessage());
        }

        boolean resultOk = ok;
        String resultMessage = message;
        plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
            try {
                controllerClient.sendJobResult(job.id, resultOk, resultMessage);
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

