package com.charles.blockwright.fabric;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.block.BlockState;
import net.minecraft.registry.Registries;
import net.minecraft.server.network.ServerPlayerEntity;
import net.minecraft.server.world.ServerWorld;
import net.minecraft.state.property.Property;
import net.minecraft.util.math.BlockPos;
import net.minecraft.util.math.Vec3d;

final class WorldScanner {
    private WorldScanner() {
    }

    static JsonModels.WorldScan scan(ServerPlayerEntity player, BlockwrightConfig config) {
        ServerWorld world = player.getWorld();
        Vec3d look = player.getRotationVec(1.0F);
        BlockPos center = player.getBlockPos().add(
                (int) Math.round(look.x * config.scanForwardBlocks),
                0,
                (int) Math.round(look.z * config.scanForwardBlocks));

        JsonModels.WorldScan scan = new JsonModels.WorldScan();
        scan.world = world.getRegistryKey().getValue().toString();
        scan.centerX = center.getX();
        scan.centerY = center.getY();
        scan.centerZ = center.getZ();
        scan.radius = config.scanRadius;
        scan.blocks = new ArrayList<>();

        int radius = config.scanRadius;
        for (int x = center.getX() - radius; x <= center.getX() + radius; x++) {
            for (int y = center.getY() - radius; y <= center.getY() + radius; y++) {
                for (int z = center.getZ() - radius; z <= center.getZ() + radius; z++) {
                    if (scan.blocks.size() >= config.maxScanBlocks) {
                        return scan;
                    }

                    BlockPos pos = new BlockPos(x, y, z);
                    var state = world.getBlockState(pos);
                    if (state.isAir()) {
                        continue;
                    }

                    JsonModels.WorldScanBlock block = new JsonModels.WorldScanBlock();
                    block.x = x;
                    block.y = y;
                    block.z = z;
                    block.material = blockStateToString(state);
                    scan.blocks.add(block);
                }
            }
        }

        return scan;
    }

    private static String blockStateToString(BlockState state) {
        String id = Registries.BLOCK.getId(state.getBlock()).toString();
        if (state.getEntries().isEmpty()) {
            return id;
        }

        List<String> entries = state.getEntries()
                .entrySet()
                .stream()
                .map(entry -> propertyEntryToString(entry.getKey(), entry.getValue()))
                .sorted()
                .toList();
        return id + "[" + String.join(",", entries) + "]";
    }

    private static <T extends Comparable<T>> String propertyEntryToString(Property<T> property, Comparable<?> value) {
        @SuppressWarnings("unchecked")
        T typedValue = (T) value;
        return property.getName() + "=" + property.name(typedValue);
    }
}
