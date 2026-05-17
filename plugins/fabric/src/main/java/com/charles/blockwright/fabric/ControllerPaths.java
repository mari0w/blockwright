package com.charles.blockwright.fabric;

import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;

final class ControllerPaths {
    private ControllerPaths() {
    }

    static String trimTrailingSlash(String value) {
        if (value == null || value.isBlank()) {
            return "http://127.0.0.1:8765";
        }

        String result = value.strip();
        while (result.endsWith("/") && result.length() > 1) {
            result = result.substring(0, result.length() - 1);
        }
        return result;
    }

    static String minecraftMessagePath() {
        return "/api/minecraft/message";
    }

    static String nextJobPath(String serverId) {
        return "/api/minecraft/jobs/next?server_id=" + encode(serverId);
    }

    static String jobResultPath(String jobId) {
        return "/api/minecraft/jobs/" + encode(jobId) + "/result";
    }

    private static String encode(String value) {
        return URLEncoder.encode(value == null ? "" : value, StandardCharsets.UTF_8);
    }
}
