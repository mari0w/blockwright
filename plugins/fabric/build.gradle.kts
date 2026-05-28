plugins {
    id("fabric-loom") version "1.10.5"
    java
}

group = "com.charles"
version = "0.1.19"

repositories {
    maven("https://maven.fabricmc.net/")
    mavenCentral()
}

val minecraftVersion = property("minecraft_version").toString()
val yarnMappings = property("yarn_mappings").toString()
val loaderVersion = property("loader_version").toString()
val fabricVersion = property("fabric_version").toString()
val blockwrightControllerBinary = providers.gradleProperty("blockwrightControllerBinary")
val blockwrightControllerClassifier = providers.gradleProperty("blockwrightControllerClassifier")
val blockwrightControllerBundleDir = providers.gradleProperty("blockwrightControllerBundleDir")

dependencies {
    minecraft("com.mojang:minecraft:$minecraftVersion")
    mappings("net.fabricmc:yarn:$yarnMappings:v2")
    modImplementation("net.fabricmc:fabric-loader:$loaderVersion")
    modImplementation("net.fabricmc.fabric-api:fabric-api:$fabricVersion")
    implementation("com.google.code.gson:gson:2.11.0")
    testImplementation(platform("org.junit:junit-bom:5.11.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

base {
    archivesName.set("blockwright-fabric")
}

java {
    withSourcesJar()
}

tasks.withType<JavaCompile> {
    options.encoding = "UTF-8"
    options.release.set(21)
}

tasks.processResources {
    filteringCharset = "UTF-8"
    inputs.property("version", project.version)
    filesMatching("fabric.mod.json") {
        expand("version" to project.version)
    }

    val controllerBundlePath = blockwrightControllerBundleDir.orNull
    if (!controllerBundlePath.isNullOrBlank()) {
        val controllerBundle = file(controllerBundlePath)
        inputs.dir(controllerBundle)
        from(controllerBundle) {
            into("blockwright/controller")
        }
    } else if (!blockwrightControllerBinary.orNull.isNullOrBlank()
        && !blockwrightControllerClassifier.orNull.isNullOrBlank()
    ) {
        val controllerBinaryPath = blockwrightControllerBinary.get()
        val controllerClassifier = blockwrightControllerClassifier.get()
        inputs.file(controllerBinaryPath)
        inputs.property("blockwrightControllerClassifier", controllerClassifier)
        from(controllerBinaryPath) {
            into("blockwright/controller/$controllerClassifier")
            rename {
                if (controllerClassifier.startsWith("windows-")) {
                    "blockwright-controller.exe"
                } else {
                    "blockwright-controller"
                }
            }
        }
    }
}

tasks.test {
    useJUnitPlatform()
}
