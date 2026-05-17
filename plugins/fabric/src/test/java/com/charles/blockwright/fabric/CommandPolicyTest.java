package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

final class CommandPolicyTest {
    @Test
    void allowsControlledMinecraftCommands() {
        assertTrue(CommandPolicy.isAllowed("time set day"));
        assertTrue(CommandPolicy.isAllowed("/weather clear"));
        assertTrue(CommandPolicy.isAllowed("gamemode creative Charles"));
        assertTrue(CommandPolicy.isAllowed("effect give Charles minecraft:night_vision 600 0 true"));
        assertTrue(CommandPolicy.isAllowed("summon minecraft:sheep ~ ~ ~"));
    }

    @Test
    void rejectsDangerousOrChainedCommands() {
        assertFalse(CommandPolicy.isAllowed("op Charles"));
        assertFalse(CommandPolicy.isAllowed("stop"));
        assertFalse(CommandPolicy.isAllowed("execute as @e run kill @s"));
        assertFalse(CommandPolicy.isAllowed("time set day; stop"));
        assertFalse(CommandPolicy.isAllowed("weather clear\nstop"));
    }

    @Test
    void normalizesSlashAndWhitespace() {
        assertEquals("time set day", CommandPolicy.normalize(" /time   set   day "));
    }
}
