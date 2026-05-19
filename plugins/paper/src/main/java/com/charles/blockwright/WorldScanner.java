package com.charles.blockwright;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.Set;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.block.data.BlockData;
import org.bukkit.entity.Player;
import org.bukkit.util.Vector;

final class WorldScanner {
    private static final int DEFAULT_RADIUS = 8;
    private static final int MAX_RADIUS = 32;
    private static final int MAX_SCAN_BLOCKS = 8000;
    private static final int SCAN_FORWARD_BLOCKS = 5;

    private WorldScanner() {
    }

    static JsonModels.WorldScan scan(Player player, int requestedRadius) {
        int radius = requestedRadius > 0 ? Math.min(requestedRadius, MAX_RADIUS) : DEFAULT_RADIUS;
        Location playerLocation = player.getLocation();
        World world = playerLocation.getWorld();
        Vector direction = playerLocation.getDirection();
        Location center = playerLocation.clone().add(
                Math.round(direction.getX() * SCAN_FORWARD_BLOCKS),
                0,
                Math.round(direction.getZ() * SCAN_FORWARD_BLOCKS));

        JsonModels.WorldScan scan = new JsonModels.WorldScan();
        scan.world = world == null ? "world" : world.getName();
        scan.centerX = center.getBlockX();
        scan.centerY = center.getBlockY();
        scan.centerZ = center.getBlockZ();
        scan.radius = radius;
        scan.blocks = new ArrayList<>();
        if (world == null) {
            return scan;
        }

        Set<String> visited = new HashSet<>();
        collectArea(world, scan, playerLocation, radius, visited);
        collectArea(world, scan, center, radius, visited);
        return scan;
    }

    private static void collectArea(
            World world,
            JsonModels.WorldScan scan,
            Location center,
            int radius,
            Set<String> visited) {
        for (int x = center.getBlockX() - radius; x <= center.getBlockX() + radius; x++) {
            for (int y = center.getBlockY() - radius; y <= center.getBlockY() + radius; y++) {
                for (int z = center.getBlockZ() - radius; z <= center.getBlockZ() + radius; z++) {
                    if (scan.blocks.size() >= MAX_SCAN_BLOCKS) {
                        return;
                    }
                    if (!visited.add(x + ":" + y + ":" + z)) {
                        continue;
                    }
                    Block block = world.getBlockAt(x, y, z);
                    if (block.isEmpty()) {
                        continue;
                    }
                    JsonModels.WorldScanBlock scanBlock = new JsonModels.WorldScanBlock();
                    scanBlock.x = x;
                    scanBlock.y = y;
                    scanBlock.z = z;
                    scanBlock.material = blockDataToString(block.getBlockData());
                    scan.blocks.add(scanBlock);
                }
            }
        }
    }

    private static String blockDataToString(BlockData blockData) {
        return blockData.getAsString();
    }
}
