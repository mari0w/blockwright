package com.charles.blockwright.fabric;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.NetworkInterface;
import java.net.SocketException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;
import java.util.concurrent.atomic.AtomicBoolean;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

final class ControllerProcessManager {
    private static final Logger LOGGER = LoggerFactory.getLogger(BlockwrightFabricMod.MOD_ID + "-controller");
    private static final String COMMAND_ENV = "BLOCKWRIGHT_CONTROLLER_COMMAND";
    private static final String WORKDIR_ENV = "BLOCKWRIGHT_CONTROLLER_WORKDIR";
    private static final AtomicBoolean STARTING = new AtomicBoolean(false);

    private static volatile Process launchedProcess;

    private ControllerProcessManager() {}

    static void ensureStartedAsync(BlockwrightConfig config, Path gameDir) {
        if (config == null || !config.autoStartController) {
            return;
        }
        Thread thread = new Thread(() -> ensureStarted(config, gameDir), "blockwright-controller-autostart");
        thread.setDaemon(true);
        thread.start();
    }

    static void restart(BlockwrightConfig config, Path gameDir) {
        synchronized (ControllerProcessManager.class) {
            stopRunningControllers(config, gameDir);
            ensureStarted(config, gameDir, true);
        }
    }

    static void stopIfLaunched() {
        Process process = launchedProcess;
        if (process == null) {
            return;
        }
        launchedProcess = null;
        if (!process.isAlive()) {
            return;
        }

        process.destroy();
        try {
            if (!process.waitFor(5, TimeUnit.SECONDS)) {
                process.destroyForcibly();
            }
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            process.destroyForcibly();
        }
    }

    private static void ensureStarted(BlockwrightConfig config, Path gameDir) {
        ensureStarted(config, gameDir, false);
    }

    private static void ensureStarted(BlockwrightConfig config, Path gameDir, boolean force) {
        if (!force && (config == null || !config.autoStartController)) {
            return;
        }
        if (launchedProcess != null && launchedProcess.isAlive()) {
            return;
        }
        if (!isLocalControllerUrl(config.controllerUrl)) {
            LOGGER.info("Blockwright controller URL is not local, skip auto-start: {}", config.controllerUrl);
            return;
        }
        if (!STARTING.compareAndSet(false, true)) {
            return;
        }

        try {
            if (isControllerHealthy(config)) {
                LOGGER.info("Blockwright controller is already running: {}", config.controllerUrl);
                logWebAddress(config);
                return;
            }

            Optional<LaunchSpec> launchSpec = resolveLaunchSpec(config, gameDir);
            if (launchSpec.isEmpty()) {
                LOGGER.warn(
                        "Blockwright controller auto-start is enabled, but no local launcher was found. "
                                + "Run scripts/install-java-mod.sh once, or set {} / {}.",
                        COMMAND_ENV,
                        WORKDIR_ENV);
                return;
            }

            LaunchSpec spec = launchSpec.get();
            Path logPath = controllerLogPath(gameDir);
            Files.createDirectories(logPath.getParent());
            ProcessBuilder builder = new ProcessBuilder(spec.command());
            if (spec.workingDirectory() != null) {
                builder.directory(spec.workingDirectory().toFile());
            }
            applyControllerEnvironment(config, builder.environment(), logPath);
            builder.redirectErrorStream(true);

            launchedProcess = builder.start();
            writeControllerPid(gameDir, launchedProcess.pid());
            streamControllerOutput(launchedProcess, logPath);
            LOGGER.info(
                    "Blockwright controller auto-started from {}. Web: {}/web, log: {}",
                    spec.source(),
                    config.controllerUrl,
                    logPath);
            if (waitUntilHealthy(config, launchedProcess)) {
                LOGGER.info("Blockwright controller is ready: {}", config.controllerUrl);
                logWebAddress(config);
            } else if (launchedProcess != null && launchedProcess.isAlive()) {
                LOGGER.warn(
                        "Blockwright controller is still starting after {} seconds. Check log: {}",
                        config.controllerStartupTimeoutSeconds,
                        logPath);
            }
        } catch (Exception error) {
            LOGGER.warn("Blockwright controller auto-start failed: {}", rootMessage(error), error);
        } finally {
            STARTING.set(false);
        }
    }

    private static void stopRunningControllers(BlockwrightConfig config, Path gameDir) {
        Map<Long, ProcessHandle> targets = new LinkedHashMap<>();
        Process process = launchedProcess;
        launchedProcess = null;
        if (process != null) {
            addTargetWithDescendants(targets, process.toHandle());
        }

        pidFileProcess(gameDir).ifPresent(handle -> addTargetWithDescendants(targets, handle));
        if (isLocalControllerUrl(config.controllerUrl)) {
            ProcessHandle.allProcesses()
                    .filter(handle -> handle.pid() != ProcessHandle.current().pid())
                    .filter(ControllerProcessManager::isControllerProcess)
                    .forEach(handle -> addTargetWithDescendants(targets, handle));
        }

        if (targets.isEmpty()) {
            LOGGER.info("No local Blockwright controller process found to stop.");
            deleteControllerPid(gameDir);
            return;
        }

        targets.values().forEach(ProcessHandle::destroy);
        for (ProcessHandle target : targets.values()) {
            waitForExit(target, 5);
        }
        for (ProcessHandle target : targets.values()) {
            if (target.isAlive()) {
                target.destroyForcibly();
                waitForExit(target, 2);
            }
        }
        deleteControllerPid(gameDir);
    }

    private static void addTargetWithDescendants(Map<Long, ProcessHandle> targets, ProcessHandle handle) {
        handle.descendants().forEach(child -> {
            if (child.pid() != ProcessHandle.current().pid()) {
                targets.put(child.pid(), child);
            }
        });
        if (handle.pid() != ProcessHandle.current().pid()) {
            targets.put(handle.pid(), handle);
        }
    }

    private static boolean isControllerProcess(ProcessHandle handle) {
        return handle
                .info()
                .commandLine()
                .map(ControllerProcessManager::isControllerCommandLineForRestart)
                .orElse(false);
    }

    static boolean isControllerCommandLineForRestart(String commandLine) {
        if (commandLine == null || commandLine.isBlank()) {
            return false;
        }
        String lower = commandLine.toLowerCase(Locale.ROOT);
        return lower.contains("blockwright-controller") && lower.contains(" serve");
    }

    private static Optional<ProcessHandle> pidFileProcess(Path gameDir) {
        Path pidPath = controllerPidPath(gameDir);
        try {
            if (!Files.isRegularFile(pidPath)) {
                return Optional.empty();
            }
            String content = Files.readString(pidPath, StandardCharsets.UTF_8).trim();
            if (content.isBlank()) {
                return Optional.empty();
            }
            long pid = Long.parseLong(content);
            return ProcessHandle.of(pid).filter(ProcessHandle::isAlive);
        } catch (IOException | NumberFormatException error) {
            LOGGER.warn("Failed to read Blockwright controller pid file {}: {}", pidPath, rootMessage(error));
            return Optional.empty();
        }
    }

    private static void writeControllerPid(Path gameDir, long pid) {
        Path pidPath = controllerPidPath(gameDir);
        try {
            Files.createDirectories(pidPath.getParent());
            Files.writeString(pidPath, Long.toString(pid), StandardCharsets.UTF_8);
        } catch (IOException error) {
            LOGGER.warn("Failed to write Blockwright controller pid file {}: {}", pidPath, rootMessage(error));
        }
    }

    private static void deleteControllerPid(Path gameDir) {
        try {
            Files.deleteIfExists(controllerPidPath(gameDir));
        } catch (IOException error) {
            LOGGER.warn("Failed to delete Blockwright controller pid file: {}", rootMessage(error));
        }
    }

    private static void waitForExit(ProcessHandle target, int timeoutSeconds) {
        try {
            target.onExit().get(timeoutSeconds, TimeUnit.SECONDS);
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
        } catch (ExecutionException | TimeoutException ignored) {
            // Caller checks target.isAlive() after the wait.
        }
    }

    static Optional<LaunchSpec> resolveLaunchSpec(BlockwrightConfig config, Path gameDir) {
        return resolveLaunchSpec(config, gameDir, System.getenv(), Path.of("").toAbsolutePath());
    }

    static Optional<LaunchSpec> resolveLaunchSpec(
            BlockwrightConfig config,
            Path gameDir,
            Map<String, String> env,
            Path processCwd) {
        Path configuredWorkDir = firstConfiguredPath(
                env.get(WORKDIR_ENV),
                config.controllerWorkingDirectory,
                gameDir);
        String configuredCommand = firstNonBlank(env.get(COMMAND_ENV), config.controllerLaunchCommand);
        if (configuredCommand != null) {
            return Optional.of(new LaunchSpec(shellCommand(configuredCommand), configuredWorkDir, "configured command"));
        }

        Optional<LaunchSpec> fromPackagedController = launchSpecFromPackagedController(gameDir);
        if (fromPackagedController.isPresent()) {
            return fromPackagedController;
        }

        if (configuredWorkDir != null) {
            Optional<LaunchSpec> fromWorkDir = launchSpecFromWorkDir(configuredWorkDir);
            if (fromWorkDir.isPresent()) {
                return fromWorkDir;
            }
        }

        Optional<LaunchSpec> fromGameDir = launchSpecFromInstalledLauncher(gameDir);
        if (fromGameDir.isPresent()) {
            return fromGameDir;
        }

        Optional<LaunchSpec> fromProcessCwd = launchSpecFromWorkDir(processCwd);
        if (fromProcessCwd.isPresent()) {
            return fromProcessCwd;
        }

        return findExecutableOnPath("blockwright-controller", env)
                .map(path -> new LaunchSpec(List.of(path.toString(), "serve"), null, "PATH blockwright-controller"));
    }

    private static Optional<LaunchSpec> launchSpecFromPackagedController(Path gameDir) {
        String classifier = currentControllerClassifier();
        if (classifier.isBlank()) {
            return Optional.empty();
        }

        String fileName = packagedControllerFileName();
        String resourcePath = "/blockwright/controller/" + classifier + "/" + fileName;
        try (InputStream input = ControllerProcessManager.class.getResourceAsStream(resourcePath)) {
            if (input == null) {
                LOGGER.info("No packaged Blockwright controller for platform {} in this mod jar.", classifier);
                return Optional.empty();
            }

            Path runtimeDir = gameDir.resolve("blockwright").resolve("runtime").resolve(classifier);
            Files.createDirectories(runtimeDir);
            Path target = runtimeDir.resolve(fileName);
            Path temp = runtimeDir.resolve(fileName + ".tmp");
            Files.copy(input, temp, StandardCopyOption.REPLACE_EXISTING);
            Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING);
            target.toFile().setExecutable(true, true);

            return Optional.of(new LaunchSpec(
                    List.of(target.toString(), "serve"),
                    gameDir.resolve("blockwright"),
                    "packaged controller " + classifier));
        } catch (IOException error) {
            LOGGER.warn("Failed to extract packaged Blockwright controller: {}", rootMessage(error), error);
            return Optional.empty();
        }
    }

    private static Optional<LaunchSpec> launchSpecFromInstalledLauncher(Path gameDir) {
        Path launcher = gameDir.resolve("blockwright").resolve(isWindows() ? "run-web.cmd" : "run-web.sh");
        if (Files.isRegularFile(launcher)) {
            return Optional.of(new LaunchSpec(scriptCommand(launcher), launcher.getParent(), "Java Edition game directory launcher"));
        }
        return Optional.empty();
    }

    private static Optional<LaunchSpec> launchSpecFromWorkDir(Path workDir) {
        if (workDir == null || !Files.isDirectory(workDir)) {
            return Optional.empty();
        }

        Path script = workDir.resolve("scripts").resolve(isWindows() ? "run-web.cmd" : "run-web.sh");
        if (Files.isRegularFile(script)) {
            return Optional.of(new LaunchSpec(scriptCommand(script), workDir, "Blockwright project scripts/run-web.sh"));
        }

        if (Files.isRegularFile(workDir.resolve("Cargo.toml")) && Files.isDirectory(workDir.resolve("apps/controller"))) {
            return Optional.of(new LaunchSpec(
                    shellCommand("cargo run -p blockwright-controller -- serve"),
                    workDir,
                    "Blockwright project cargo fallback"));
        }

        return Optional.empty();
    }

    private static List<String> scriptCommand(Path script) {
        if (isWindows()) {
            return List.of("cmd", "/c", script.toString());
        }
        return List.of("/bin/sh", script.toString());
    }

    private static List<String> shellCommand(String command) {
        if (isWindows()) {
            return List.of("cmd", "/c", command);
        }
        return List.of("/bin/sh", "-lc", command);
    }

    private static Optional<Path> findExecutableOnPath(String name, Map<String, String> env) {
        String pathValue = env.get("PATH");
        if (pathValue == null || pathValue.isBlank()) {
            return Optional.empty();
        }

        String executable = isWindows() ? name + ".exe" : name;
        for (String entry : pathValue.split(java.io.File.pathSeparator)) {
            if (entry.isBlank()) {
                continue;
            }
            Path candidate = Path.of(entry).resolve(executable);
            if (Files.isRegularFile(candidate) && Files.isExecutable(candidate)) {
                return Optional.of(candidate);
            }
        }
        return Optional.empty();
    }

    private static void applyControllerEnvironment(BlockwrightConfig config, Map<String, String> env, Path logPath) {
        env.putIfAbsent("SERVER_NAME", "local");
        env.putIfAbsent("BLOCKWRIGHT_AUTOSTART", "1");
        env.putIfAbsent("BLOCKWRIGHT_CONTROLLER_LOG_PATH", logPath.toString());
        controllerPort(config.controllerUrl).ifPresent(port -> env.putIfAbsent("PORT", Integer.toString(port)));
    }

    private static void streamControllerOutput(Process process, Path logPath) {
        Thread thread = new Thread(() -> {
            try (BufferedReader reader = new BufferedReader(new InputStreamReader(
                            process.getInputStream(),
                            StandardCharsets.UTF_8));
                    BufferedWriter writer = Files.newBufferedWriter(
                            logPath,
                            StandardCharsets.UTF_8,
                            StandardOpenOption.CREATE,
                            StandardOpenOption.APPEND)) {
                String line;
                while ((line = reader.readLine()) != null) {
                    writer.write(line);
                    writer.newLine();
                    writer.flush();
                    LOGGER.info("controller | {}", line);
                }
            } catch (IOException error) {
                LOGGER.warn("Blockwright controller log stream ended with error: {}", rootMessage(error));
            }
        }, "blockwright-controller-output");
        thread.setDaemon(true);
        thread.start();
    }

    private static Optional<Integer> controllerPort(String controllerUrl) {
        try {
            URI uri = URI.create(controllerUrl);
            int port = uri.getPort();
            if (port > 0) {
                return Optional.of(port);
            }
            if ("https".equalsIgnoreCase(uri.getScheme())) {
                return Optional.of(443);
            }
            if ("http".equalsIgnoreCase(uri.getScheme())) {
                return Optional.of(80);
            }
        } catch (IllegalArgumentException ignored) {
            return Optional.empty();
        }
        return Optional.empty();
    }

    private static boolean isControllerHealthy(BlockwrightConfig config) {
        try {
            HttpClient client = HttpClient.newBuilder()
                    .connectTimeout(Duration.ofSeconds(Math.max(1L, config.connectTimeoutSeconds)))
                    .build();
            HttpRequest request = HttpRequest.newBuilder(URI.create(config.controllerUrl + "/health"))
                    .timeout(Duration.ofSeconds(Math.max(1L, config.connectTimeoutSeconds)))
                    .GET()
                    .build();
            HttpResponse<String> response = client.send(request, HttpResponse.BodyHandlers.ofString());
            return response.statusCode() >= 200 && response.statusCode() < 300;
        } catch (IOException | InterruptedException | IllegalArgumentException error) {
            if (error instanceof InterruptedException) {
                Thread.currentThread().interrupt();
            }
            return false;
        }
    }

    private static boolean waitUntilHealthy(BlockwrightConfig config, Process process) throws InterruptedException {
        long deadline = System.nanoTime() + Duration.ofSeconds(config.controllerStartupTimeoutSeconds).toNanos();
        while (System.nanoTime() < deadline) {
            if (isControllerHealthy(config)) {
                return true;
            }
            if (process != null && !process.isAlive()) {
                LOGGER.warn("Blockwright controller process exited early with code {}", process.exitValue());
                return false;
            }
            Thread.sleep(1000L);
        }
        return false;
    }

    static Path controllerLogPath(Path gameDir) {
        return gameDir.resolve("logs").resolve("blockwright-controller.log");
    }

    static Path controllerPidPath(Path gameDir) {
        return gameDir.resolve("blockwright").resolve("controller.pid");
    }

    private static boolean isLocalControllerUrl(String controllerUrl) {
        try {
            URI uri = URI.create(controllerUrl);
            String host = uri.getHost();
            if (host == null) {
                return false;
            }
            String normalized = host.toLowerCase(Locale.ROOT);
            return normalized.equals("localhost")
                    || normalized.equals("0.0.0.0")
                    || normalized.equals("::")
                    || normalized.equals("::1")
                    || normalized.equals("127.0.0.1")
                    || normalized.startsWith("127.");
        } catch (IllegalArgumentException error) {
            return false;
        }
    }

    private static Path firstConfiguredPath(String envValue, String configValue, Path baseDir) {
        String value = firstNonBlank(envValue, configValue);
        if (value == null) {
            return null;
        }
        return configuredPath(value, baseDir);
    }

    private static Path configuredPath(String value, Path baseDir) {
        String normalized = value.trim();
        String home = System.getProperty("user.home", "");
        if (normalized.equals("~")) {
            normalized = home;
        } else if (normalized.startsWith("~/") || normalized.startsWith("~\\")) {
            normalized = home + normalized.substring(1);
        }
        Path path = Path.of(normalized);
        if (path.isAbsolute()) {
            return path.normalize();
        }
        return baseDir.resolve(path).normalize();
    }

    private static String firstNonBlank(String first, String second) {
        if (first != null && !first.isBlank()) {
            return first.trim();
        }
        if (second != null && !second.isBlank()) {
            return second.trim();
        }
        return null;
    }

    private static boolean isWindows() {
        return System.getProperty("os.name", "").toLowerCase(Locale.ROOT).contains("win");
    }

    private static String currentControllerClassifier() {
        return controllerClassifier(
                System.getProperty("os.name", ""),
                System.getProperty("os.arch", ""));
    }

    static String controllerClassifier(String osName, String archName) {
        osName = osName.toLowerCase(Locale.ROOT);
        String os;
        if (osName.contains("mac") || osName.contains("darwin")) {
            os = "macos";
        } else if (osName.contains("linux")) {
            os = "linux";
        } else if (osName.contains("win")) {
            os = "windows";
        } else {
            return "";
        }

        archName = archName.toLowerCase(Locale.ROOT);
        String arch;
        if (archName.equals("aarch64") || archName.equals("arm64")) {
            arch = "aarch64";
        } else if (archName.equals("x86_64") || archName.equals("amd64")) {
            arch = "x86_64";
        } else {
            return "";
        }

        return os + "-" + arch;
    }

    private static String packagedControllerFileName() {
        return isWindows() ? "blockwright-controller.exe" : "blockwright-controller";
    }

    private static void logWebAddress(BlockwrightConfig config) {
        Optional<String> lanIp = primaryLanIpv4();
        List<String> messages = webAddressMessages(config.controllerUrl, lanIp, BlockwrightLanguage.ENGLISH);
        LOGGER.info(messages.get(0));
        if (lanIp.isPresent()) {
            LOGGER.info(messages.get(1));
        } else {
            LOGGER.warn(messages.get(1));
        }
    }

    static List<String> webAddressMessages(String controllerUrl) {
        return webAddressMessages(controllerUrl, primaryLanIpv4(), BlockwrightLanguage.ENGLISH);
    }

    static List<String> webAddressMessages(String controllerUrl, BlockwrightLanguage language) {
        return webAddressMessages(controllerUrl, primaryLanIpv4(), language);
    }

    static List<String> startupHintMessages(BlockwrightConfig config) {
        return startupHintMessages(config, BlockwrightLanguage.ENGLISH);
    }

    static List<String> startupHintMessages(BlockwrightConfig config, BlockwrightLanguage language) {
        String controllerUrl = config == null ? "http://127.0.0.1:8765" : config.controllerUrl;
        boolean autoStart = config == null || config.autoStartController;
        return startupHintMessages(controllerUrl, autoStart, primaryLanIpv4(), language);
    }

    static List<String> startupHintMessages(String controllerUrl, boolean autoStart, Optional<String> lanIp) {
        return startupHintMessages(controllerUrl, autoStart, lanIp, BlockwrightLanguage.ENGLISH);
    }

    static List<String> startupHintMessages(
            String controllerUrl,
            boolean autoStart,
            Optional<String> lanIp,
            BlockwrightLanguage language) {
        List<String> messages = new ArrayList<>();
        messages.add(language.text("Blockwright Web: ", "Blockwright Web：")
                + loopbackWebAddress(controllerUrl));
        lanIp.ifPresent(ip -> messages.add(language.text("LAN Web: ", "局域网 Web：")
                + lanWebAddress(controllerUrl, ip)));
        messages.add(language.text(
                "Open this page to finish setup.",
                "打开这个页面完成配置。"));
        return messages;
    }

    static List<String> webAddressMessages(String controllerUrl, Optional<String> lanIp) {
        return webAddressMessages(controllerUrl, lanIp, BlockwrightLanguage.ENGLISH);
    }

    static List<String> webAddressMessages(
            String controllerUrl,
            Optional<String> lanIp,
            BlockwrightLanguage language) {
        List<String> messages = new ArrayList<>();
        messages.add(language.text("Blockwright local Web address: ", "Blockwright 本机 Web 地址：")
                + loopbackWebAddress(controllerUrl));
        if (lanIp.isPresent()) {
            messages.add(language.text("Blockwright LAN Web address: ", "Blockwright 局域网 Web 地址：")
                    + lanWebAddress(controllerUrl, lanIp.get()));
        } else {
            messages.add(language.text(
                    "Blockwright did not detect a LAN IPv4 address; use the local Web address on this computer.",
                    "Blockwright 没有检测到局域网 IPv4 地址；只能使用本机 Web 地址。"));
        }
        return messages;
    }

    static String loopbackWebAddress(String controllerUrl) {
        try {
            URI uri = URI.create(controllerUrl);
            String scheme = uri.getScheme() == null || uri.getScheme().isBlank() ? "http" : uri.getScheme();
            int port = controllerPort(controllerUrl).orElse(8765);
            return scheme + "://127.0.0.1:" + port + "/web";
        } catch (IllegalArgumentException error) {
            return "http://127.0.0.1:8765/web";
        }
    }

    static String lanWebAddress(String controllerUrl, String lanIp) {
        try {
            URI uri = URI.create(controllerUrl);
            String scheme = uri.getScheme() == null || uri.getScheme().isBlank() ? "http" : uri.getScheme();
            int port = controllerPort(controllerUrl).orElse(8765);
            return scheme + "://" + lanIp + ":" + port + "/web";
        } catch (IllegalArgumentException error) {
            return "http://" + lanIp + ":8765/web";
        }
    }

    private static Optional<String> primaryLanIpv4() {
        Optional<String> routedAddress = routedLanIpv4();
        if (routedAddress.isPresent()) {
            return routedAddress;
        }
        return firstInterfaceLanIpv4();
    }

    private static Optional<String> routedLanIpv4() {
        try (DatagramSocket socket = new DatagramSocket()) {
            socket.connect(InetAddress.getByName("8.8.8.8"), 80);
            InetAddress address = socket.getLocalAddress();
            if (!isUsableIpv4(address)) {
                return Optional.empty();
            }
            return Optional.of(address.getHostAddress());
        } catch (IOException error) {
            return Optional.empty();
        }
    }

    private static Optional<String> firstInterfaceLanIpv4() {
        String fallback = null;
        try {
            Enumeration<NetworkInterface> interfaces = NetworkInterface.getNetworkInterfaces();
            if (interfaces == null) {
                return Optional.empty();
            }
            while (interfaces.hasMoreElements()) {
                NetworkInterface networkInterface = interfaces.nextElement();
                if (!networkInterface.isUp() || networkInterface.isLoopback() || networkInterface.isVirtual()) {
                    continue;
                }
                Enumeration<InetAddress> addresses = networkInterface.getInetAddresses();
                while (addresses.hasMoreElements()) {
                    InetAddress address = addresses.nextElement();
                    if (!isUsableIpv4(address)) {
                        continue;
                    }
                    if (address.isSiteLocalAddress()) {
                        return Optional.of(address.getHostAddress());
                    }
                    if (fallback == null) {
                        fallback = address.getHostAddress();
                    }
                }
            }
        } catch (SocketException error) {
            return Optional.empty();
        }
        return Optional.ofNullable(fallback);
    }

    private static boolean isUsableIpv4(InetAddress address) {
        if (address == null
                || address.isAnyLocalAddress()
                || address.isLoopbackAddress()
                || address.isLinkLocalAddress()) {
            return false;
        }
        String hostAddress = address.getHostAddress();
        return hostAddress != null && !hostAddress.contains(":");
    }

    private static String rootMessage(Throwable error) {
        Throwable current = error;
        while (current.getCause() != null) {
            current = current.getCause();
        }
        return current.getMessage() == null ? current.getClass().getSimpleName() : current.getMessage();
    }

    record LaunchSpec(List<String> command, Path workingDirectory, String source) {
        LaunchSpec {
            command = List.copyOf(new ArrayList<>(command));
        }
    }
}
