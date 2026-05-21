plugins {
    id("fabric-loom") version "1.10.5"
    java
}

group = "com.charles"
version = "0.1.6"

repositories {
    maven("https://maven.fabricmc.net/")
    mavenCentral()
}

val minecraftVersion = property("minecraft_version").toString()
val yarnMappings = property("yarn_mappings").toString()
val loaderVersion = property("loader_version").toString()
val fabricVersion = property("fabric_version").toString()

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
}

tasks.test {
    useJUnitPlatform()
}
