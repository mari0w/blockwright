package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

final class ControllerPathsTest {
    @Test
    void trimTrailingSlashKeepsCleanBaseUrl() {
        assertEquals("http://127.0.0.1:8765", ControllerPaths.trimTrailingSlash("http://127.0.0.1:8765/"));
        assertEquals("http://127.0.0.1:8765", ControllerPaths.trimTrailingSlash(" http://127.0.0.1:8765// "));
        assertEquals("http://127.0.0.1:8765", ControllerPaths.trimTrailingSlash(""));
    }

    @Test
    void buildsControllerApiPathsWithEncodedIds() {
        assertEquals("/api/minecraft/message", ControllerPaths.minecraftMessagePath());
        assertEquals(
                "/api/minecraft/progress/req-1%2Fbad",
                ControllerPaths.minecraftProgressPath("req-1/bad"));
        assertEquals(
                "/api/minecraft/jobs/next?server_id=hmcl-lan",
                ControllerPaths.nextJobPath("hmcl-lan"));
        assertEquals(
                "/api/minecraft/jobs/hm-job-1%2Fbad/result",
                ControllerPaths.jobResultPath("hm-job-1/bad"));
    }
}
