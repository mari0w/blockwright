package com.charles.blockwright.fabric;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.channels.SeekableByteChannel;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

final class ControllerLogViewer {
    private static final int RECENT_TAIL_BYTES = 256 * 1024;
    private static final int WATCH_TAIL_BYTES = 128 * 1024;
    private static final int MAX_CHAT_LINE_CHARS = 260;

    private ControllerLogViewer() {}

    static List<String> recentLines(Path logPath, boolean includeAll, int maxLines) throws IOException {
        List<String> lines = splitLines(readTailText(logPath, RECENT_TAIL_BYTES));
        return lastMatchingLines(lines, includeAll, maxLines);
    }

    static TailResult readSince(Path logPath, long previousPosition, boolean includeAll, int maxLines)
            throws IOException {
        long fileSize = Files.size(logPath);
        long start = previousPosition;
        if (start < 0 || start > fileSize) {
            start = Math.max(0, fileSize - WATCH_TAIL_BYTES);
        }
        if (start == fileSize) {
            return new TailResult(List.of(), fileSize);
        }

        boolean truncatedStart = false;
        int maxBytes = (int) Math.min(WATCH_TAIL_BYTES, fileSize - start);
        if (fileSize - start > WATCH_TAIL_BYTES) {
            start = fileSize - WATCH_TAIL_BYTES;
            truncatedStart = true;
        }

        String text;
        try (SeekableByteChannel channel = Files.newByteChannel(logPath, StandardOpenOption.READ)) {
            channel.position(start);
            ByteBuffer buffer = ByteBuffer.allocate(maxBytes);
            while (buffer.hasRemaining() && channel.read(buffer) > 0) {
                // Continue until EOF or the capped buffer is full.
            }
            buffer.flip();
            text = StandardCharsets.UTF_8.decode(buffer).toString();
        }

        List<String> lines = splitLines(text);
        if (truncatedStart && start > 0 && !text.startsWith("\n") && !text.startsWith("\r")) {
            lines = lines.isEmpty() ? lines : lines.subList(1, lines.size());
        }
        return new TailResult(lastMatchingLines(lines, includeAll, maxLines), fileSize);
    }

    static long currentSize(Path logPath) throws IOException {
        return Files.size(logPath);
    }

    private static String readTailText(Path logPath, int maxBytes) throws IOException {
        long fileSize = Files.size(logPath);
        long start = Math.max(0, fileSize - maxBytes);
        int byteCount = (int) (fileSize - start);
        try (SeekableByteChannel channel = Files.newByteChannel(logPath, StandardOpenOption.READ)) {
            channel.position(start);
            ByteBuffer buffer = ByteBuffer.allocate(byteCount);
            while (buffer.hasRemaining() && channel.read(buffer) > 0) {
                // Continue until EOF or the capped buffer is full.
            }
            buffer.flip();
            String text = StandardCharsets.UTF_8.decode(buffer).toString();
            if (start > 0) {
                int firstNewline = firstNewlineIndex(text);
                if (firstNewline >= 0 && firstNewline + 1 < text.length()) {
                    return text.substring(firstNewline + 1);
                }
            }
            return text;
        }
    }

    private static int firstNewlineIndex(String text) {
        int lf = text.indexOf('\n');
        int cr = text.indexOf('\r');
        if (lf < 0) {
            return cr;
        }
        if (cr < 0) {
            return lf;
        }
        return Math.min(lf, cr);
    }

    private static List<String> splitLines(String text) {
        if (text == null || text.isBlank()) {
            return List.of();
        }
        String[] rawLines = text.split("\\R");
        List<String> lines = new ArrayList<>();
        for (String rawLine : rawLines) {
            String line = formatForChat(rawLine);
            if (!line.isBlank()) {
                lines.add(line);
            }
        }
        return lines;
    }

    private static List<String> lastMatchingLines(List<String> lines, boolean includeAll, int maxLines) {
        List<String> matches = new ArrayList<>();
        for (String line : lines) {
            if (includeAll || isModelLogLine(line)) {
                matches.add(line);
            }
        }
        int start = Math.max(0, matches.size() - Math.max(1, maxLines));
        return List.copyOf(matches.subList(start, matches.size()));
    }

    private static boolean isModelLogLine(String line) {
        String lower = line.toLowerCase(Locale.ROOT);
        return lower.contains("received minecraft message")
                || lower.contains("handled robot message")
                || lower.contains("starting codex unified planner")
                || lower.contains("prompt prepared")
                || lower.contains("starting llm api request")
                || lower.contains("finished llm api request")
                || lower.contains("starting gemini api request")
                || lower.contains("finished gemini api request")
                || lower.contains("starting codex cli request")
                || lower.contains("finished codex cli request")
                || lower.contains("codex cli progress event")
                || lower.contains("planned with codex unified planner")
                || lower.contains("planned minecraft message");
    }

    private static String formatForChat(String rawLine) {
        String line = stripAnsi(rawLine).trim();
        line = line.replace('\t', ' ');
        int infoIndex = line.indexOf(" INFO ");
        int warnIndex = line.indexOf(" WARN ");
        int errorIndex = line.indexOf(" ERROR ");
        int levelIndex = firstPositive(infoIndex, warnIndex, errorIndex);
        if (levelIndex > 0 && levelIndex + 1 < line.length()) {
            line = line.substring(0, Math.min(19, levelIndex)).trim() + " " + line.substring(levelIndex).trim();
        }
        if (line.length() > MAX_CHAT_LINE_CHARS) {
            line = line.substring(0, MAX_CHAT_LINE_CHARS - 3) + "...";
        }
        return line;
    }

    private static int firstPositive(int... values) {
        int result = -1;
        for (int value : values) {
            if (value >= 0 && (result < 0 || value < result)) {
                result = value;
            }
        }
        return result;
    }

    private static String stripAnsi(String value) {
        return value.replaceAll("\u001B\\[[;\\d]*m", "");
    }

    record TailResult(List<String> lines, long nextPosition) {}
}
