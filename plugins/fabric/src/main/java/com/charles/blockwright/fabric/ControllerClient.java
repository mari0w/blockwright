package com.charles.blockwright.fabric;

import com.google.gson.Gson;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;

public final class ControllerClient {
    private final HttpClient httpClient;
    private final Gson gson;
    private final BlockwrightConfig config;

    public ControllerClient(BlockwrightConfig config) {
        this.httpClient = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(config.connectTimeoutSeconds))
                .build();
        this.gson = new Gson();
        this.config = config;
    }

    public interface ProgressListener {
        void onProgress(JsonModels.ProgressSnapshot progress);
    }

    public JsonModels.MinecraftMessageResponse sendMinecraftMessage(
            PlayerSnapshot player,
            String text,
            JsonModels.WorldScan nearbyScan)
            throws IOException, InterruptedException {
        return sendMinecraftMessage(player, text, nearbyScan, null);
    }

    public JsonModels.MinecraftMessageResponse sendMinecraftMessage(
            PlayerSnapshot player,
            String text,
            JsonModels.WorldScan nearbyScan,
            List<JsonModels.ChatAttachment> attachments)
            throws IOException, InterruptedException {
        return sendMinecraftMessage(player, text, nearbyScan, attachments, null);
    }

    public JsonModels.MinecraftMessageResponse sendMinecraftMessage(
            PlayerSnapshot player,
            String text,
            JsonModels.WorldScan nearbyScan,
            List<JsonModels.ChatAttachment> attachments,
            ProgressListener progressListener)
            throws IOException, InterruptedException {
        JsonModels.MinecraftMessageRequest request = new JsonModels.MinecraftMessageRequest();
        request.serverId = config.serverId;
        request.player = player.name();
        request.text = text;
        request.position = player.position();
        request.nearbyScan = nearbyScan;
        request.attachments = attachments;
        request.progressId = "fabric-" + UUID.randomUUID();

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
                    .timeout(Duration.ofSeconds(Math.max(2L, config.connectTimeoutSeconds)))
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
        HttpRequest httpRequest = baseRequest(ControllerPaths.nextJobPath(config.serverId)).GET().build();
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

        HttpRequest httpRequest = baseRequest(ControllerPaths.jobResultPath(jobId))
                .POST(HttpRequest.BodyPublishers.ofString(gson.toJson(request)))
                .build();
        HttpResponse<String> response = httpClient.send(httpRequest, HttpResponse.BodyHandlers.ofString());
        ensureSuccess(response);
    }

    private HttpRequest.Builder baseRequest(String path) {
        return HttpRequest.newBuilder(URI.create(config.controllerUrl + path))
                .timeout(Duration.ofSeconds(config.requestTimeoutSeconds))
                .header("Content-Type", "application/json")
                .header("X-Blockwright-Token", config.sharedToken);
    }

    private void ensureSuccess(HttpResponse<String> response) throws IOException {
        if (response.statusCode() < 200 || response.statusCode() >= 300) {
            throw new IOException("controller returned " + response.statusCode() + ": " + response.body());
        }
    }
}
