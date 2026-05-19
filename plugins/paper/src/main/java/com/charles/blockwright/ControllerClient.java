package com.charles.blockwright;

import com.google.gson.Gson;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import org.bukkit.Location;
import org.bukkit.entity.Player;

public final class ControllerClient {
    static final long REQUEST_TIMEOUT_SECONDS = 30 * 60L;

    private final BlockwrightPlugin plugin;
    private final HttpClient httpClient;
    private final Gson gson;
    private final String controllerUrl;
    private final String serverId;
    private final String sharedToken;
    private final long connectTimeoutSeconds;
    private final long requestTimeoutSeconds;

    public ControllerClient(BlockwrightPlugin plugin) {
        this.plugin = plugin;
        this.connectTimeoutSeconds = plugin.getConfig().getLong("connect-timeout-seconds", 5L);
        this.requestTimeoutSeconds = normalizeRequestTimeout(
                plugin.getConfig().getLong("request-timeout-seconds", REQUEST_TIMEOUT_SECONDS));
        this.httpClient = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(connectTimeoutSeconds))
                .build();
        this.gson = new Gson();
        this.controllerUrl =
                ControllerPaths.trimTrailingSlash(plugin.getConfig().getString("controller-url", "http://127.0.0.1:8765"));
        this.serverId = plugin.getConfig().getString("server-id", "local-paper");
        this.sharedToken = plugin.getConfig().getString("shared-token", "local-dev-token");
    }

    public interface ProgressListener {
        void onProgress(JsonModels.ProgressSnapshot progress);
    }

    public JsonModels.MinecraftMessageResponse sendMinecraftMessage(Player player, String text)
            throws IOException, InterruptedException {
        return sendMinecraftMessage(player, text, null);
    }

    public JsonModels.MinecraftMessageResponse sendMinecraftMessage(
            Player player,
            String text,
            ProgressListener progressListener)
            throws IOException, InterruptedException {
        Location location = player.getLocation();
        JsonModels.MinecraftMessageRequest request = new JsonModels.MinecraftMessageRequest();
        request.serverId = serverId;
        request.player = player.getName();
        request.text = text;
        request.position = JsonModels.PlayerPosition.fromLocation(location);
        request.playerState = JsonModels.PlayerState.fromPlayer(player);
        request.progressId = "paper-" + UUID.randomUUID();

        HttpRequest httpRequest = baseRequest(ControllerPaths.minecraftMessagePath())
                .POST(HttpRequest.BodyPublishers.ofString(gson.toJson(request)))
                .build();
        CompletableFuture<HttpResponse<String>> pendingResponse =
                httpClient.sendAsync(httpRequest, HttpResponse.BodyHandlers.ofString());
        HttpResponse<String> response =
                waitForResponseWithProgress(pendingResponse, request.progressId, progressListener);
        ensureSuccess(response);
        return gson.fromJson(response.body(), JsonModels.MinecraftMessageResponse.class);
    }

    private HttpResponse<String> waitForResponseWithProgress(
            CompletableFuture<HttpResponse<String>> pendingResponse,
            String progressId,
            ProgressListener progressListener)
            throws IOException, InterruptedException {
        long lastSequence = 0L;
        while (!pendingResponse.isDone()) {
            Thread.sleep(1000L);
            lastSequence = pollProgress(progressId, lastSequence, progressListener);
        }
        lastSequence = pollProgress(progressId, lastSequence, progressListener);
        try {
            return pendingResponse.get();
        } catch (ExecutionException error) {
            Throwable cause = error.getCause();
            if (cause instanceof IOException ioError) {
                throw ioError;
            }
            if (cause instanceof InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                throw interrupted;
            }
            throw new IOException(cause == null ? error : cause);
        }
    }

    private long pollProgress(
            String progressId,
            long lastSequence,
            ProgressListener progressListener)
            throws InterruptedException {
        if (progressListener == null || progressId == null || progressId.isBlank()) {
            return lastSequence;
        }

        try {
            HttpRequest httpRequest = baseRequest(ControllerPaths.minecraftProgressPath(progressId))
                    .timeout(Duration.ofSeconds(Math.max(2L, connectTimeoutSeconds)))
                    .GET()
                    .build();
            HttpResponse<String> response = httpClient.send(httpRequest, HttpResponse.BodyHandlers.ofString());
            if (response.statusCode() < 200 || response.statusCode() >= 300) {
                return lastSequence;
            }
            JsonModels.ProgressSnapshot progress =
                    gson.fromJson(response.body(), JsonModels.ProgressSnapshot.class);
            if (progress != null && progress.sequence > lastSequence) {
                progressListener.onProgress(progress);
                return progress.sequence;
            }
        } catch (IOException error) {
            return lastSequence;
        }
        return lastSequence;
    }

    public JsonModels.NextJobResponse nextJob() throws IOException, InterruptedException {
        HttpRequest httpRequest = baseRequest(ControllerPaths.nextJobPath(serverId)).GET().build();
        HttpResponse<String> response = httpClient.send(httpRequest, HttpResponse.BodyHandlers.ofString());
        ensureSuccess(response);
        return gson.fromJson(response.body(), JsonModels.NextJobResponse.class);
    }

    public void sendJobResult(
            String jobId,
            boolean ok,
            String message,
            JsonModels.JobExecutionReport report)
            throws IOException, InterruptedException {
        JsonModels.JobResultRequest request = new JsonModels.JobResultRequest();
        request.ok = ok;
        request.message = message;
        request.report = report;
        sendJobResult(jobId, request);
    }

    public void sendJobResult(String jobId, JsonModels.JobResultRequest request)
            throws IOException, InterruptedException {
        HttpRequest httpRequest = baseRequest(ControllerPaths.jobResultPath(jobId))
                .POST(HttpRequest.BodyPublishers.ofString(gson.toJson(request)))
                .build();
        HttpResponse<String> response = httpClient.send(httpRequest, HttpResponse.BodyHandlers.ofString());
        ensureSuccess(response);
    }

    private HttpRequest.Builder baseRequest(String path) {
        return HttpRequest.newBuilder(URI.create(controllerUrl + path))
                .timeout(Duration.ofSeconds(requestTimeoutSeconds))
                .header("Content-Type", "application/json")
                .header("X-Blockwright-Token", sharedToken);
    }

    private void ensureSuccess(HttpResponse<String> response) throws IOException {
        if (response.statusCode() < 200 || response.statusCode() >= 300) {
            throw new IOException("controller returned " + response.statusCode() + ": " + response.body());
        }
    }

    static long normalizeRequestTimeout(long value) {
        return REQUEST_TIMEOUT_SECONDS;
    }
}
