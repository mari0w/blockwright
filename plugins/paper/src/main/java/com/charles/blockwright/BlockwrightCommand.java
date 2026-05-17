package com.charles.blockwright;

import java.util.Arrays;
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
        if (args.length == 1 && args[0].equalsIgnoreCase("reload")) {
            plugin.reloadBlockwrightConfig();
            sender.sendMessage("Blockwright 配置已重新加载。");
            return true;
        }

        if (args.length < 2 || !args[0].equalsIgnoreCase("ask")) {
            sender.sendMessage("用法：/bw ask <你想要的物品或建筑>");
            return true;
        }

        if (!(sender instanceof Player player)) {
            sender.sendMessage("这个命令需要玩家在游戏内执行。");
            return true;
        }

        String text = String.join(" ", Arrays.copyOfRange(args, 1, args.length));
        sender.sendMessage("Blockwright 正在处理你的需求...");

        plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
            try {
                JsonModels.MinecraftMessageResponse response =
                        controllerClient.sendMinecraftMessage(player, text);
                plugin.getServer().getScheduler().runTask(plugin, () -> {
                    player.sendMessage(response.reply);
                    Location origin = player.getLocation();
                    actionExecutor.executeActions(response.actions, player.getName(), origin);
                });
            } catch (Exception error) {
                plugin.getLogger().warning("controller request failed: " + error.getMessage());
                plugin.getServer().getScheduler().runTask(plugin,
                        () -> sender.sendMessage("Blockwright controller 请求失败：" + error.getMessage()));
            }
        });

        return true;
    }
}

