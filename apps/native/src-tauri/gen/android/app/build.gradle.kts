import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

// The app version is the workspace version in the root Cargo.toml (ADR 0012). The Tauri CLI does not
// carry it over to Android by itself, so read it here. versionCode is major * 1000000 + minor * 1000
// + patch, the formula Tauri uses, so it always rises with the version.
val workspaceVersion: String = run {
    val manifest = file("../../../../../../Cargo.toml").readText()
    val section = manifest.substringAfter("[workspace.package]", "")
    Regex("""(?m)^version\s*=\s*"(\d+\.\d+\.\d+)"""").find(section)?.groupValues?.get(1)
        ?: throw GradleException("No version = \"x.y.z\" under [workspace.package] in the root Cargo.toml")
}
val workspaceVersionCode: Int = workspaceVersion.split(".").map { it.toInt() }
    .let { (major, minor, patch) -> major * 1000000 + minor * 1000 + patch }

android {
    compileSdk = 37
    namespace = "io.github.emobe.flashcards"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "io.github.emobe.flashcards"
        minSdk = 24
        targetSdk = 37
        versionCode = workspaceVersionCode
        versionName = workspaceVersion
    }
    buildTypes {
        getByName("debug") {
            applicationIdSuffix = ".dev"
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
