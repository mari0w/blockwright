package com.charles.blockwright.fabric;

import com.google.gson.Gson;
import com.google.gson.annotations.SerializedName;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;

final class ControllerLocalConfigClient {
    private static final Gson GSON = new Gson();

    private ControllerLocalConfigClient() {}

    static String saveMatrixConfig(BlockwrightConfig config) throws IOException, InterruptedException {
        MatrixLocalConfigRequest request = new MatrixLocalConfigRequest();
        request.enabled = config.matrixEnabled;
        request.homeserverUrl = config.matrixHomeserverUrl;
        request.accessToken = config.matrixAccessToken;
        request.allowedSender = config.matrixAllowedSender;
        request.allowOwnUserMessages = config.matrixAllowOwnUserMessages;
        request.autoJoinInvites = config.matrixAutoJoinInvites;
        request.defaultServerId = config.serverId;
        request.defaultTargetPlayer = config.matrixDefaultTargetPlayer;

        HttpClient client = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(config.connectTimeoutSeconds))
                .build();
        HttpRequest httpRequest = HttpRequest.newBuilder(URI.create(config.controllerUrl + "/api/chat/matrix/local-config"))
                .timeout(Duration.ofSeconds(config.requestTimeoutSeconds))
                .header("Content-Type", "application/json")
                .header("X-Blockwright-Token", config.sharedToken)
                .PUT(HttpRequest.BodyPublishers.ofString(GSON.toJson(request)))
                .build();
        HttpResponse<String> response = client.send(httpRequest, HttpResponse.BodyHandlers.ofString());
        if (response.statusCode() < 200 || response.statusCode() >= 300) {
            throw new IOException("controller returned " + response.statusCode() + ": " + response.body());
        }
        MatrixLocalConfigResponse body = GSON.fromJson(response.body(), MatrixLocalConfigResponse.class);
        if (body == null || body.message == null || body.message.isBlank()) {
            return "Matrix/Element 配置已保存到 controller。";
        }
        return body.message;
    }

    private static final class MatrixLocalConfigRequest {
        boolean enabled;
        @SerializedName("homeserver_url")
        String homeserverUrl;
        @SerializedName("access_token")
        String accessToken;
        @SerializedName("allowed_sender")
        String allowedSender;
        @SerializedName("allow_own_user_messages")
        boolean allowOwnUserMessages;
        @SerializedName("auto_join_invites")
        boolean autoJoinInvites;
        @SerializedName("default_server_id")
        String defaultServerId;
        @SerializedName("default_target_player")
        String defaultTargetPlayer;
    }

    private static final class MatrixLocalConfigResponse {
        String message;
    }
}
