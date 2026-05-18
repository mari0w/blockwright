package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import org.junit.jupiter.api.Test;

final class BlockMaterialSpecTest {
    @Test
    void parsesPlainAndStatefulBlockMaterials() {
        ActionExecutor.BlockMaterialSpec plain = ActionExecutor.BlockMaterialSpec.parse("minecraft:oak_planks");
        assertEquals("minecraft:oak_planks", plain.id());
        assertEquals(0, plain.states().size());

        ActionExecutor.BlockMaterialSpec leaves = ActionExecutor.BlockMaterialSpec.parse(
                "minecraft:oak_leaves[persistent=true,distance=1]");
        assertEquals("minecraft:oak_leaves", leaves.id());
        assertEquals("true", leaves.states().get("persistent"));
        assertEquals("1", leaves.states().get("distance"));
    }

    @Test
    void rejectsMalformedBlockStates() {
        assertThrows(IllegalArgumentException.class, () -> ActionExecutor.BlockMaterialSpec.parse("minecraft:oak_leaves["));
        assertThrows(IllegalArgumentException.class, () -> ActionExecutor.BlockMaterialSpec.parse("minecraft:oak_leaves[persistent]"));
        assertThrows(IllegalArgumentException.class, () -> ActionExecutor.BlockMaterialSpec.parse("minecraft:oak_leaves[persistent=true,persistent=false]"));
    }
}
