package com.charles.blockwright;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

final class ControllerClientTest {
    @Test
    void normalizesRequestTimeoutToThirtyMinutes() {
        assertEquals(1800L, ControllerClient.normalizeRequestTimeout(5L));
        assertEquals(1800L, ControllerClient.normalizeRequestTimeout(180L));
        assertEquals(1800L, ControllerClient.normalizeRequestTimeout(9999L));
    }
}
