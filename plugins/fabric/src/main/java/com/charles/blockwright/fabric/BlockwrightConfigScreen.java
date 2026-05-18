package com.charles.blockwright.fabric;

import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.gui.widget.ButtonWidget;
import net.minecraft.client.gui.widget.TextFieldWidget;
import net.minecraft.text.Text;

final class BlockwrightConfigScreen extends Screen {
    private final Screen parent;
    private final Path configPath;
    private final BlockwrightConfig config;
    private final List<LabeledField> fields = new ArrayList<>();

    private TextFieldWidget controllerUrlField;
    private TextFieldWidget sharedTokenField;
    private TextFieldWidget serverIdField;
    private TextFieldWidget matrixHomeserverField;
    private TextFieldWidget matrixAccessTokenField;
    private TextFieldWidget matrixAllowedSenderField;
    private TextFieldWidget matrixDefaultTargetPlayerField;
    private ButtonWidget matrixEnabledButton;
    private ButtonWidget allowOwnMessagesButton;
    private Text status = Text.literal("");
    private boolean matrixEnabled;
    private boolean allowOwnMessages;

    BlockwrightConfigScreen(Screen parent, Path configPath, BlockwrightConfig config) {
        super(Text.literal("Blockwright 配置"));
        this.parent = parent;
        this.configPath = configPath;
        this.config = config;
        this.matrixEnabled = config.matrixEnabled;
        this.allowOwnMessages = config.matrixAllowOwnUserMessages;
    }

    @Override
    protected void init() {
        fields.clear();
        int formWidth = Math.min(420, this.width - 40);
        int x = (this.width - formWidth) / 2;
        int y = 42;
        controllerUrlField = addField("Controller 地址", config.controllerUrl, x, y, formWidth);
        y += 42;
        sharedTokenField = addField("Controller Token", config.sharedToken, x, y, formWidth);
        y += 42;
        serverIdField = addField("服务器 ID", config.serverId, x, y, formWidth);
        y += 42;
        matrixHomeserverField = addField("Matrix 主服务器 URL", config.matrixHomeserverUrl, x, y, formWidth);
        y += 42;
        matrixAccessTokenField = addField("Matrix Access Token", config.matrixAccessToken, x, y, formWidth);
        y += 42;
        matrixAllowedSenderField = addField("允许发指令的 Matrix 用户", config.matrixAllowedSender, x, y, formWidth);
        y += 34;
        matrixDefaultTargetPlayerField = addField("Element 默认目标玩家", config.matrixDefaultTargetPlayer, x, y, formWidth);
        y += 34;

        matrixEnabledButton = ButtonWidget.builder(Text.empty(), button -> {
                    matrixEnabled = !matrixEnabled;
                    refreshToggleLabels();
                })
                .dimensions(x, y, 200, 20)
                .build();
        addDrawableChild(matrixEnabledButton);
        allowOwnMessagesButton = ButtonWidget.builder(Text.empty(), button -> {
                    allowOwnMessages = !allowOwnMessages;
                    refreshToggleLabels();
                })
                .dimensions(x + formWidth - 200, y, 200, 20)
                .build();
        addDrawableChild(allowOwnMessagesButton);
        refreshToggleLabels();
        y += 32;

        addDrawableChild(ButtonWidget.builder(Text.literal("保存并应用到 controller"), button -> save())
                .dimensions(x, y, 200, 20)
                .build());
        addDrawableChild(ButtonWidget.builder(Text.literal("取消"), button -> close())
                .dimensions(x + formWidth - 90, y, 90, 20)
                .build());
    }

    private TextFieldWidget addField(String label, String value, int x, int y, int width) {
        TextFieldWidget field = new TextFieldWidget(textRenderer, x, y, width, 20, Text.literal(label));
        field.setMaxLength(4096);
        field.setText(value == null ? "" : value);
        fields.add(new LabeledField(label, field, y - 12));
        addDrawableChild(field);
        return field;
    }

    private void refreshToggleLabels() {
        if (matrixEnabledButton != null) {
            matrixEnabledButton.setMessage(Text.literal("Element 接入：" + (matrixEnabled ? "启用" : "禁用")));
        }
        if (allowOwnMessagesButton != null) {
            allowOwnMessagesButton.setMessage(Text.literal("个人 token：" + (allowOwnMessages ? "允许" : "跳过自己")));
        }
    }

    private void save() {
        status = Text.literal("正在保存...");
        BlockwrightConfig next = readConfigFromFields();
        CompletableFuture.supplyAsync(() -> {
                    try {
                        BlockwrightConfig.save(configPath, next);
                        return ControllerLocalConfigClient.saveMatrixConfig(next);
                    } catch (Exception error) {
                        throw new IllegalStateException(rootMessage(error), error);
                    }
                })
                .thenAccept(message -> MinecraftClient.getInstance()
                        .execute(() -> status = Text.literal(message)))
                .exceptionally(error -> {
                    MinecraftClient.getInstance()
                            .execute(() -> status = Text.literal("保存失败：" + rootMessage(error)));
                    return null;
                });
    }

    private BlockwrightConfig readConfigFromFields() {
        BlockwrightConfig next = new BlockwrightConfig();
        next.controllerUrl = controllerUrlField.getText();
        next.sharedToken = sharedTokenField.getText();
        next.serverId = serverIdField.getText();
        next.connectTimeoutSeconds = config.connectTimeoutSeconds;
        next.requestTimeoutSeconds = config.requestTimeoutSeconds;
        next.protectExistingBlocks = config.protectExistingBlocks;
        next.maxBlocksPerAction = config.maxBlocksPerAction;
        next.scanRadius = config.scanRadius;
        next.scanForwardBlocks = config.scanForwardBlocks;
        next.maxScanBlocks = config.maxScanBlocks;
        next.pollControllerJobs = config.pollControllerJobs;
        next.pollIntervalTicks = config.pollIntervalTicks;
        next.matrixEnabled = matrixEnabled;
        next.matrixHomeserverUrl = matrixHomeserverField.getText();
        next.matrixAccessToken = matrixAccessTokenField.getText();
        next.matrixAllowedSender = matrixAllowedSenderField.getText();
        next.matrixDefaultTargetPlayer = matrixDefaultTargetPlayerField.getText();
        next.matrixAllowOwnUserMessages = allowOwnMessages;
        next.matrixAutoJoinInvites = config.matrixAutoJoinInvites;
        return next;
    }

    @Override
    public void render(DrawContext context, int mouseX, int mouseY, float delta) {
        super.render(context, mouseX, mouseY, delta);
        context.drawCenteredTextWithShadow(textRenderer, title, width / 2, 18, 0xFFFFFF);
        for (LabeledField field : fields) {
            context.drawTextWithShadow(textRenderer, field.label, field.widget.getX(), field.labelY, 0xA0A0A0);
        }
        context.drawCenteredTextWithShadow(textRenderer, status, width / 2, height - 28, 0xE0E0E0);
    }

    @Override
    public void close() {
        if (client != null) {
            client.setScreen(parent);
        }
    }

    private static String rootMessage(Throwable error) {
        Throwable current = error;
        while (current.getCause() != null) {
            current = current.getCause();
        }
        return current.getMessage() == null ? current.getClass().getSimpleName() : current.getMessage();
    }

    private record LabeledField(String label, TextFieldWidget widget, int labelY) {}
}
