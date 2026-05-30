plugins {
    java
}

group = "com.charles"
version = "0.1.3"

dependencies {
    compileOnly("io.papermc.paper:paper-api:26.1.2.build.66-stable")
    compileOnly("com.google.code.gson:gson:2.11.0")
    testImplementation("io.papermc.paper:paper-api:26.1.2.build.66-stable")
    testImplementation(platform("org.junit:junit-bom:5.11.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}

tasks.withType<JavaCompile> {
    options.encoding = "UTF-8"
    options.release.set(21)
}

tasks.test {
    useJUnitPlatform()
}

tasks.processResources {
    filteringCharset = "UTF-8"
    filesMatching("plugin.yml") {
        expand("version" to project.version)
    }
}
