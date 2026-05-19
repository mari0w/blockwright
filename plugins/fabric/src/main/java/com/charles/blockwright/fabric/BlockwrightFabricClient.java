package com.charles.blockwright.fabric;

import net.fabricmc.api.ClientModInitializer;

public final class BlockwrightFabricClient implements ClientModInitializer {
    @Override
    public void onInitializeClient() {
        // 配置入口统一放到 Web 端，这里不再注册 /bwconfig。
    }
}
