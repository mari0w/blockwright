package com.charles.blockwright.fabric;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import org.junit.jupiter.api.Test;

final class ControllerProcessManagerTest {
    @Test
    void resolvesInstalledHmclLauncherFromGameDirectory() throws Exception {
        Path gameDir = Files.createTempDirectory("blockwright-game-dir");
        Path launcher = gameDir.resolve("blockwright").resolve("run-web.sh");
        Files.createDirectories(launcher.getParent());
        Files.writeString(launcher, "#!/usr/bin/env bash\n");

        ControllerProcessManager.LaunchSpec spec = ControllerProcessManager
                .resolveLaunchSpec(new BlockwrightConfig(), gameDir, Map.of(), gameDir)
                .orElseThrow();

        assertEquals(launcher.getParent(), spec.workingDirectory());
        assertTrue(spec.command().contains(launcher.toString()));
        assertEquals("Java Edition game directory launcher", spec.source());
    }

    @Test
    void configuredCommandTakesPrecedenceOverInstalledLauncher() throws Exception {
        Path gameDir = Files.createTempDirectory("blockwright-game-dir");
        Path launcher = gameDir.resolve("blockwright").resolve("run-web.sh");
        Files.createDirectories(launcher.getParent());
        Files.writeString(launcher, "#!/usr/bin/env bash\n");

        BlockwrightConfig config = new BlockwrightConfig();
        config.controllerLaunchCommand = "echo custom";
        config.controllerWorkingDirectory = ".";

        ControllerProcessManager.LaunchSpec spec = ControllerProcessManager
                .resolveLaunchSpec(config, gameDir, Map.of(), gameDir)
                .orElseThrow();

        assertEquals(gameDir, spec.workingDirectory());
        assertTrue(spec.command().contains("echo custom"));
        assertEquals("configured command", spec.source());
    }

    @Test
    void resolvesProjectRunWebScriptFromConfiguredWorkDir() throws Exception {
        Path gameDir = Files.createTempDirectory("blockwright-game-dir");
        Path projectDir = Files.createTempDirectory("blockwright-project-dir");
        Path script = projectDir.resolve("scripts").resolve("run-web.sh");
        Files.createDirectories(script.getParent());
        Files.writeString(script, "#!/usr/bin/env bash\n");

        BlockwrightConfig config = new BlockwrightConfig();
        config.controllerWorkingDirectory = projectDir.toString();

        ControllerProcessManager.LaunchSpec spec = ControllerProcessManager
                .resolveLaunchSpec(config, gameDir, Map.of(), gameDir)
                .orElseThrow();

        assertEquals(projectDir, spec.workingDirectory());
        assertTrue(spec.command().contains(script.toString()));
        assertEquals("Blockwright project scripts/run-web.sh", spec.source());
    }

    @Test
    void mapsCommonDesktopPlatformsToPackagedControllerClassifiers() {
        assertEquals("macos-aarch64", ControllerProcessManager.controllerClassifier("Mac OS X", "aarch64"));
        assertEquals("macos-x86_64", ControllerProcessManager.controllerClassifier("Mac OS X", "x86_64"));
        assertEquals("linux-aarch64", ControllerProcessManager.controllerClassifier("Linux", "arm64"));
        assertEquals("linux-x86_64", ControllerProcessManager.controllerClassifier("Linux", "amd64"));
        assertEquals("windows-x86_64", ControllerProcessManager.controllerClassifier("Windows 11", "amd64"));
        assertEquals("", ControllerProcessManager.controllerClassifier("Solaris", "sparc"));
    }

    @Test
    void formatsTerminalWebAddressesForLocalAndLanAccess() {
        assertEquals(
                "http://127.0.0.1:8765/web",
                ControllerProcessManager.loopbackWebAddress("http://0.0.0.0:8765"));
        assertEquals(
                "http://192.168.5.155:8765/web",
                ControllerProcessManager.lanWebAddress("http://127.0.0.1:8765", "192.168.5.155"));
        assertEquals(
                "http://192.168.5.155:8765/web",
                ControllerProcessManager.lanWebAddress("bad-url", "192.168.5.155"));
    }

    @Test
    void identifiesControllerProcessesForRestart() {
        assertTrue(ControllerProcessManager.isControllerCommandLineForRestart(
                "/Applications/.minecraft/blockwright/runtime/macos-aarch64/blockwright-controller serve"));
        assertTrue(ControllerProcessManager.isControllerCommandLineForRestart(
                "SCREEN -dmS blockwright-controller /bin/zsh -lc blockwright-controller serve"));
        assertFalse(ControllerProcessManager.isControllerCommandLineForRestart(
                "java -jar minecraft-client.jar"));
        assertFalse(ControllerProcessManager.isControllerCommandLineForRestart(
                "blockwright-controller --help"));
    }

    @Test
    void storesControllerPidInsideGameDirectory() throws Exception {
        Path gameDir = Files.createTempDirectory("blockwright-game-dir");

        assertEquals(
                gameDir.resolve("blockwright").resolve("controller.pid"),
                ControllerProcessManager.controllerPidPath(gameDir));
    }

    @Test
    void buildsWebAddressMessagesForCommandOutput() {
        assertEquals(
                List.of(
                        "Blockwright local Web address: http://127.0.0.1:8765/web",
                        "Blockwright LAN Web address: http://192.168.5.155:8765/web"),
                ControllerProcessManager.webAddressMessages(
                        "http://127.0.0.1:8765",
                        Optional.of("192.168.5.155")));
        assertEquals(
                List.of(
                        "Blockwright local Web address: http://127.0.0.1:8765/web",
                        "Blockwright did not detect a LAN IPv4 address; use the local Web address on this computer."),
                ControllerProcessManager.webAddressMessages("http://127.0.0.1:8765", Optional.empty()));
    }

    @Test
    void buildsChineseWebAddressMessagesWhenRequested() {
        assertEquals(
                List.of(
                        "Blockwright 本机 Web 地址：http://127.0.0.1:8765/web",
                        "Blockwright 局域网 Web 地址：http://192.168.5.155:8765/web"),
                ControllerProcessManager.webAddressMessages(
                        "http://127.0.0.1:8765",
                        Optional.of("192.168.5.155"),
                        BlockwrightLanguage.CHINESE));
    }

    @Test
    void buildsStartupHintMessagesForJoinedPlayers() {
        assertEquals(
                List.of(
                        "Blockwright Web: http://127.0.0.1:8765/web",
                        "LAN Web: http://192.168.5.155:8765/web",
                        "Open this page to finish setup."),
                ControllerProcessManager.startupHintMessages(
                        "http://127.0.0.1:8765",
                        true,
                        Optional.of("192.168.5.155")));

        assertEquals(
                List.of(
                        "Blockwright Web: http://127.0.0.1:8765/web",
                        "Open this page to finish setup."),
                ControllerProcessManager.startupHintMessages(
                        "http://127.0.0.1:8765",
                        false,
                        Optional.empty()));
    }

    @Test
    void buildsChineseStartupHintMessagesWhenRequested() {
        assertEquals(
                List.of(
                        "Blockwright Web：http://127.0.0.1:8765/web",
                        "局域网 Web：http://192.168.5.155:8765/web",
                        "打开这个页面完成配置。"),
                ControllerProcessManager.startupHintMessages(
                        "http://127.0.0.1:8765",
                        true,
                        Optional.of("192.168.5.155"),
                        BlockwrightLanguage.CHINESE));
    }
}
