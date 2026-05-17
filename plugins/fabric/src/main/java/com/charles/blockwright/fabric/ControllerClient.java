package com.charles.blockwright.fabric;

import com.google.gson.Gson;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;

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

    public JsonModels.MinecraftMessageResponse sendMinecraftMessage(
            PlayerSnapshot player,
            String text,
            JsonModels.WorldScan nearbyScan)
            throws IOException, InterruptedException {
        JsonModels.MinecraftMessageRequest request = new JsonModels.MinecraftMessageRequest();
        request.serverId = config.serverId;
        request.player = player.name();
        request.text = text;
        request.position = player.position();
        request.nearbyScan = nearbyScan;

        HttpRequest httpRequest = baseRequest(ControllerPaths.minecraftMessagePath())
                .POST(HttpRequest.BodyPublishers.ofString(gson.toJson(request)))
                .build();
        HttpResponse<String> response = httpClient.send(httpRequest, HttpResponse.BodyHandlers.ofString());
        ensureSuccess(response);
        return gson.fromJson(response.body(), JsonModels.MinecraftMessageResponse.class);
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
