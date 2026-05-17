package com.charles.blockwright;

import org.bukkit.command.PluginCommand;
import org.bukkit.plugin.java.JavaPlugin;

public final class BlockwrightPlugin extends JavaPlugin {
    private ControllerClient controllerClient;
    private ActionExecutor actionExecutor;
    private JobPoller jobPoller;

    @Override
    public void onEnable() {
        saveDefaultConfig();
        controllerClient = new ControllerClient(this);
        actionExecutor = new ActionExecutor(this);
        registerCommand();

        jobPoller = new JobPoller(this, controllerClient, actionExecutor);
        jobPoller.start();
        getLogger().info("Blockwright enabled");
    }

    @Override
    public void onDisable() {
        if (jobPoller != null) {
            jobPoller.cancel();
        }
    }

    public void reloadBlockwrightConfig() {
        reloadConfig();
        controllerClient = new ControllerClient(this);
        actionExecutor = new ActionExecutor(this);
        if (jobPoller != null) {
            jobPoller.cancel();
        }
        jobPoller = new JobPoller(this, controllerClient, actionExecutor);
        jobPoller.start();
        registerCommand();
    }

    private void registerCommand() {
        PluginCommand command = getCommand("bw");
        if (command != null) {
            command.setExecutor(new BlockwrightCommand(this, controllerClient, actionExecutor));
        }
    }
}
