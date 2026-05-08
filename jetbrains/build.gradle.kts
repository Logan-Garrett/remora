plugins {
    id("org.jetbrains.intellij.platform") version "2.2.1"
    kotlin("jvm") version "2.0.0"
}

group = "com.remora"
version = "0.9.3"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        intellijIdeaCommunity("2024.1")
        instrumentationTools()
    }
    implementation("io.ktor:ktor-client-websockets:3.0.0")
    implementation("io.ktor:ktor-client-cio:3.0.0")
    implementation("com.google.code.gson:gson:2.11.0")
}

intellijPlatform {
    pluginConfiguration {
        id = "com.remora.plugin"
        name = "Remora"
        version = project.version.toString()
        description = "Collaborative Claude Code sessions in JetBrains IDEs"
        changeNotes = "Initial release"
        ideaVersion {
            sinceBuild = "241"
        }
    }
}

tasks {
    compileKotlin { kotlinOptions.jvmTarget = "17" }
}
