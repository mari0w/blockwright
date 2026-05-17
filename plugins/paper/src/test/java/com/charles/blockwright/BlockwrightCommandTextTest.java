package com.charles.blockwright;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

final class BlockwrightCommandTextTest {
    @Test
    void detectsReloadCommand() {
        assertTrue(BlockwrightCommandText.isReload(new String[] {"reload"}));
    }

    @Test
    void extractsAskChatAndDirectText() {
        assertEquals("帮我盖一个木屋", BlockwrightCommandText.extractChatText(new String[] {"ask", "帮我盖一个木屋"}));
        assertEquals("给我钻石", BlockwrightCommandText.extractChatText(new String[] {"chat", "给我钻石"}));
        assertEquals("给我一把钻石剑", BlockwrightCommandText.extractChatText(new String[] {"给我一把钻石剑"}));
    }

    @Test
    void returnsNullForIncompleteCommand() {
        assertNull(BlockwrightCommandText.extractChatText(new String[] {}));
        assertNull(BlockwrightCommandText.extractChatText(new String[] {"ask"}));
        assertNull(BlockwrightCommandText.extractChatText(new String[] {"reload"}));
    }
}
