package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

final class BlockwrightLanguageTest {
    @Test
    void defaultsToEnglishForMissingOrNonChineseCodes() {
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromLanguageCode(null));
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromLanguageCode(""));
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromLanguageCode("en_us"));
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromLanguageCode("ja_jp"));
    }

    @Test
    void parsesExplicitChineseLanguageCodes() {
        assertEquals(BlockwrightLanguage.CHINESE, BlockwrightLanguage.fromLanguageCode("zh_cn"));
        assertEquals(BlockwrightLanguage.CHINESE, BlockwrightLanguage.fromLanguageCode("zh_tw"));
    }

    @Test
    void missingPlayersAndCommandSourcesDefaultToEnglish() {
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromPlayer(null));
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromSource(null));
    }
}
