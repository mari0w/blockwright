package com.charles.blockwright;

import java.util.ArrayList;
import java.util.List;
import org.bukkit.Location;
import org.bukkit.entity.Player;
import org.bukkit.scheduler.BukkitTask;

public final class JobPoller {
    private final BlockwrightPlugin plugin;
    private final ControllerClient controllerClient;
    private final ActionExecutor actionExecutor;
    private BukkitTask task;
    private volatile RunningJob runningJob;

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
        RunningJob active = runningJob;
        if (active != null) {
            active.cancel();
            runningJob = null;
        }
    }

    private void pollOnce() {
        if (runningJob != null) {
            return;
        }

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
            BlockwrightLanguage language = BlockwrightLanguage.fromPlayer(resolveTargetPlayer(job.targetPlayer));
            if (executeLiveQueryJob(job)) {
                return;
            }
            Location origin = defaultOrigin(job.targetPlayer);
            if (hasPlaceBlocks(job.actions)) {
                if (startControlledActions(job.id, job.targetPlayer, job.summary, job.actions, origin)) {
                    return;
                }
                ok = false;
                message = language.text(
                        "The executor is busy, so the build did not start.",
                        "执行端正忙，建筑任务未开始。");
            } else {
                report = actionExecutor.executeActions(job.actions, job.targetPlayer, origin);
                ok = report.isOk();
                if (!ok) {
                    message = language.text(
                            "Build execution failed; execution report returned",
                            "建筑执行失败，已回传执行报告");
                }
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

    public boolean startControlledActions(
            String jobId,
            String targetPlayer,
            String summary,
            List<JsonModels.GameAction> actions,
            Location origin) {
        if (!hasPlaceBlocks(actions) || runningJob != null) {
            return false;
        }

        JsonModels.GameJob job = new JsonModels.GameJob();
        job.id = jobId;
        job.targetPlayer = targetPlayer;
        job.summary = summary;
        job.actions = actions;
        RunningJob next = new RunningJob(job, origin);
        runningJob = next;
        next.start();
        return true;
    }

    public static boolean hasPlaceBlocks(List<JsonModels.GameAction> actions) {
        if (actions == null) {
            return false;
        }
        for (JsonModels.GameAction action : actions) {
            if (action != null && "place_blocks".equals(action.type)) {
                return true;
            }
        }
        return false;
    }

    private boolean executeLiveQueryJob(JsonModels.GameJob job) {
        JsonModels.GameAction stateAction = firstAction(job.actions, "get_player_state");
        if (stateAction != null) {
            Player player = resolveTargetPlayer(job.targetPlayer);
            JsonModels.JobResultRequest result = new JsonModels.JobResultRequest();
            result.ok = player != null;
            result.message = player == null
                    ? "No online player is available for this query"
                    : "ok";
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
            result.message = player == null
                    ? "No online player is available for this scan"
                    : "ok";
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

    private final class RunningJob {
        private final JsonModels.GameJob job;
        private final Location origin;
        private final BlockwrightLanguage language;
        private final JsonModels.JobExecutionReport report = new JsonModels.JobExecutionReport();
        private int actionIndex;
        private BukkitTask actionTask;

        RunningJob(JsonModels.GameJob job, Location origin) {
            this.job = job;
            this.origin = origin;
            this.language = BlockwrightLanguage.fromPlayer(resolveTargetPlayer(job.targetPlayer));
            this.report.actions = new ArrayList<>();
        }

        void start() {
            actionTask = plugin.getServer().getScheduler().runTaskTimer(plugin, this::step, 1L, 1L);
        }

        void cancel() {
            if (actionTask != null) {
                actionTask.cancel();
                actionTask = null;
            }
        }

        private void step() {
            try {
                if (job.actions != null && actionIndex < job.actions.size()) {
                    JsonModels.GameAction action = job.actions.get(actionIndex);
                    if (action != null) {
                        JsonModels.JobExecutionReport stepReport =
                                actionExecutor.executeActions(List.of(action), job.targetPlayer, origin);
                        if (stepReport != null && stepReport.actions != null) {
                            report.actions.addAll(stepReport.actions);
                        }
                    }
                    actionIndex++;
                    if (actionIndex < job.actions.size()) {
                        return;
                    }
                }

                boolean ok = report.isOk();
                finish(ok, ok
                        ? "ok"
                        : language.text(
                                "Build execution failed; execution report returned",
                                "建筑执行失败，已回传执行报告"));
            } catch (Exception error) {
                plugin.getLogger().warning("chunked job execute failed: " + job.id + ", " + error.getMessage());
                finish(false, error.getMessage());
            }
        }

        private void finish(boolean ok, String message) {
            cancel();
            JsonModels.JobExecutionReport resultReport = report;
            if (job.id != null && !job.id.isBlank()) {
                plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
                    try {
                        controllerClient.sendJobResult(job.id, ok, message, resultReport);
                    } catch (Exception error) {
                        plugin.getLogger().warning("send job result failed: " + error.getMessage());
                    }
                });
            }
            runningJob = null;
        }
    }
}
