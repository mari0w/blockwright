package com.charles.blockwright.fabric;

import java.util.ArrayList;
import java.util.List;
import net.minecraft.entity.player.PlayerInventory;
import net.minecraft.item.ItemStack;
import net.minecraft.registry.Registries;
import net.minecraft.server.network.ServerPlayerEntity;

public record PlayerSnapshot(String name, JsonModels.PlayerPosition position, JsonModels.PlayerState playerState) {
    public static PlayerSnapshot from(ServerPlayerEntity player) {
        return new PlayerSnapshot(
                player.getName().getString(),
                JsonModels.PlayerPosition.fromPlayer(player),
                playerState(player));
    }

    private static JsonModels.PlayerState playerState(ServerPlayerEntity player) {
        PlayerInventory inventory = player.getInventory();
        JsonModels.PlayerState state = new JsonModels.PlayerState();
        state.clientLanguage = player.getClientOptions().language();
        state.selectedSlot = inventory.getSelectedSlot();
        state.mainHand = itemStack(inventory.getStack(state.selectedSlot));
        state.offHand = itemStack(player.getOffHandStack());
        state.inventory = new ArrayList<>();
        for (int slot = 0; slot < inventory.size(); slot++) {
            ItemStack stack = inventory.getStack(slot);
            JsonModels.PlayerItemStack item = itemStack(stack);
            if (item == null) {
                continue;
            }
            JsonModels.PlayerInventorySlot inventorySlot = new JsonModels.PlayerInventorySlot();
            inventorySlot.slot = slot;
            inventorySlot.item = item.item;
            inventorySlot.count = item.count;
            inventorySlot.hotbar = slot < 9;
            inventorySlot.selected = slot == state.selectedSlot;
            state.inventory.add(inventorySlot);
        }
        return state;
    }

    private static JsonModels.PlayerItemStack itemStack(ItemStack stack) {
        if (stack == null || stack.isEmpty()) {
            return null;
        }
        JsonModels.PlayerItemStack item = new JsonModels.PlayerItemStack();
        item.item = Registries.ITEM.getId(stack.getItem()).toString();
        item.count = stack.getCount();
        return item;
    }
}
