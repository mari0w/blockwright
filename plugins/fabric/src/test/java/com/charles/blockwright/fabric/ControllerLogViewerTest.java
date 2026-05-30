package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;

final class ControllerLogViewerTest {
    @Test
    void recentLinesDefaultToModelRelatedLogs() throws Exception {
        Path log = Files.createTempFile("blockwright-controller", ".log");
        Files.writeString(
                log,
                """
                2026-05-29T15:32:43Z INFO unrelated startup line
                \u001B[32m2026-05-29T15:32:43Z INFO blockwright_controller::services::planner: codex unified planner prompt prepared prompt_bytes=631742\u001B[0m
                2026-05-29T15:32:43Z INFO blockwright_controller::integrations::llm: starting llm api request provider="OpenAI API" model=gpt-4.1
                2026-05-29T15:32:44Z INFO blockwright_controller::http::minecraft: planned minecraft message action_count=1
                """);

        List<String> lines = ControllerLogViewer.recentLines(log, false, 10);

        assertEquals(3, lines.size());
        assertTrue(lines.get(0).contains("prompt prepared"));
        assertTrue(!lines.get(0).contains("\u001B"));
        assertTrue(lines.get(1).contains("model=gpt-4.1"));
        assertTrue(lines.get(2).contains("planned minecraft message"));
    }

    @Test
    void readSinceReturnsOnlyNewFilteredLinesAndNextPosition() throws Exception {
        Path log = Files.createTempFile("blockwright-controller", ".log");
        Files.writeString(log, "old line\n");
        long offset = Files.size(log);
        Files.writeString(
                log,
                """
                2026-05-29T15:34:19Z INFO blockwright_controller::integrations::llm: finished llm api request provider="OpenAI API" model=gpt-4.1
                2026-05-29T15:34:20Z INFO unrelated line
                """,
                java.nio.file.StandardOpenOption.APPEND);

        ControllerLogViewer.TailResult result = ControllerLogViewer.readSince(log, offset, false, 10);

        assertEquals(Files.size(log), result.nextPosition());
        assertEquals(1, result.lines().size());
        assertTrue(result.lines().get(0).contains("finished llm api request"));
    }

    @Test
    void includeAllReturnsRawRecentLogTail() throws Exception {
        Path log = Files.createTempFile("blockwright-controller", ".log");
        Files.writeString(log, "first\nsecond\nthird\n");

        assertEquals(List.of("second", "third"), ControllerLogViewer.recentLines(log, true, 2));
    }
}
