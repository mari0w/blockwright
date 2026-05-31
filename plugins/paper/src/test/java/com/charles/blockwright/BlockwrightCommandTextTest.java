package com.charles.blockwright;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

final class BlockwrightCommandTextTest {
    @Test
    void detectsWebCommand() {
        assertTrue(BlockwrightCommandText.isWeb(new String[] {"web"}));
        assertTrue(BlockwrightCommandText.isWeb(new String[] {"WEB"}));
    }

    @Test
    void extractsDirectText() {
        assertEquals("给我一把钻石剑", BlockwrightCommandText.extractChatText(new String[] {"给我一把钻石剑"}));
        assertEquals("build a creeper house", BlockwrightCommandText.extractChatText(new String[] {"build", "a", "creeper", "house"}));
    }

    @Test
    void returnsNullForIncompleteCommand() {
        assertNull(BlockwrightCommandText.extractChatText(new String[] {}));
        assertNull(BlockwrightCommandText.extractChatText(new String[] {"web"}));
    }

    @Test
    void usageDefaultsToEnglishAndSupportsChinese() {
        assertEquals("Usage: /bw <request>, or /bw web", BlockwrightCommandText.usage());
        assertEquals(
                "用法：/bw <需求>，或 /bw web",
                BlockwrightCommandText.usage(BlockwrightLanguage.CHINESE));
    }
}
