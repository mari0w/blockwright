package com.charles.blockwright;

import static org.junit.jupiter.api.Assertions.assertEquals;

import java.lang.reflect.Proxy;
import java.util.Locale;
import org.bukkit.entity.Player;
import org.junit.jupiter.api.Test;

final class BlockwrightLanguageTest {
    @Test
    void defaultsToEnglishForMissingOrNonChineseLocales() {
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromLocale(null));
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromLocale(Locale.ENGLISH));
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromLocale(Locale.JAPANESE));
    }

    @Test
    void detectsChineseLocales() {
        assertEquals(BlockwrightLanguage.CHINESE, BlockwrightLanguage.fromLocale(Locale.CHINA));
        assertEquals(BlockwrightLanguage.CHINESE, BlockwrightLanguage.fromLocale(Locale.TAIWAN));
    }

    @Test
    void playersAndSendersFollowPlayerLocale() {
        Player player = playerWithLocale(Locale.CHINA);

        assertEquals(BlockwrightLanguage.CHINESE, BlockwrightLanguage.fromPlayer(player));
        assertEquals(BlockwrightLanguage.CHINESE, BlockwrightLanguage.fromSender(player));
        assertEquals(BlockwrightLanguage.ENGLISH, BlockwrightLanguage.fromSender(null));
    }

    private static Player playerWithLocale(Locale locale) {
        return (Player) Proxy.newProxyInstance(
                Player.class.getClassLoader(),
                new Class<?>[] {Player.class},
                (proxy, method, args) -> {
                    if (method.getDeclaringClass() == Object.class) {
                        return switch (method.getName()) {
                            case "toString" -> "Player(" + locale + ")";
                            case "hashCode" -> System.identityHashCode(proxy);
                            case "equals" -> proxy == args[0];
                            default -> null;
                        };
                    }
                    if (method.getName().equals("locale")) {
                        return locale;
                    }
                    return defaultValue(method.getReturnType());
                });
    }

    private static Object defaultValue(Class<?> returnType) {
        if (!returnType.isPrimitive()) {
            return null;
        }
        if (returnType == boolean.class) {
            return false;
        }
        if (returnType == char.class) {
            return '\0';
        }
        if (returnType == byte.class) {
            return (byte) 0;
        }
        if (returnType == short.class) {
            return (short) 0;
        }
        if (returnType == int.class) {
            return 0;
        }
        if (returnType == long.class) {
            return 0L;
        }
        if (returnType == float.class) {
            return 0F;
        }
        if (returnType == double.class) {
            return 0D;
        }
        return null;
    }
}
