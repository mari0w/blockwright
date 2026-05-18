package com.charles.blockwright.fabric;

import java.nio.file.Path;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandManager;
import net.fabricmc.fabric.api.client.command.v2.ClientCommandRegistrationCallback;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.MinecraftClient;
import net.minecraft.text.Text;

public final class BlockwrightFabricClient implements ClientModInitializer {
    private static int openConfigDelayTicks = -1;

    @Override
    public void onInitializeClient() {
        ClientCommandRegistrationCallback.EVENT.register((dispatcher, registryAccess) -> {
            dispatcher.register(ClientCommandManager.literal("bwconfig")
                    .executes(context -> scheduleConfigScreen()));
            dispatcher.register(ClientCommandManager.literal("bw")
                    .then(ClientCommandManager.literal("config")
                            .executes(context -> scheduleConfigScreen())));
        });
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            if (openConfigDelayTicks < 0) {
                return;
            }
            if (openConfigDelayTicks-- > 0) {
                return;
            }
            openConfigDelayTicks = -1;
            openConfigScreen(client, null);
        });
    }

    private static int scheduleConfigScreen() {
        openConfigDelayTicks = 2;
        MinecraftClient client = MinecraftClient.getInstance();
        if (client.player != null) {
            client.player.sendMessage(Text.literal("正在打开 Blockwright 配置界面..."), false);
        }
        return 1;
    }

    static void openConfigScreen(MinecraftClient client, net.minecraft.client.gui.screen.Screen parent) {
        Path path = FabricLoader.getInstance().getConfigDir().resolve("blockwright.json");
        try {
            BlockwrightConfig config = BlockwrightConfig.load(path);
            client.setScreen(new BlockwrightConfigScreen(parent, path, config));
        } catch (Exception error) {
            if (client.player != null) {
                client.player.sendMessage(Text.literal("Blockwright 配置打开失败：" + error.getMessage()), false);
            }
        }
    }
}
