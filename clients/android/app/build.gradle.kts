import groovy.json.JsonSlurper

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.rstorrent.bootstrap"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "com.jstorrent.rstorrent"
        minSdk = 28
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            abiFilters += listOf("x86_64", "arm64-v8a")
        }
    }

    signingConfigs {
        create("release") {
            val keyPath = providers.environmentVariable("UPLOAD_KEYSTORE_PATH").orNull
            if (keyPath != null) {
                storeFile = file(keyPath)
                storePassword = providers.environmentVariable("UPLOAD_KEYSTORE_PASSWORD").orNull
                keyAlias = providers.environmentVariable("UPLOAD_KEY_ALIAS").orNull
                keyPassword = providers.environmentVariable("UPLOAD_KEY_PASSWORD").orNull
            }
        }
    }

    buildTypes {
        debug {
            isDebuggable = true
            isPseudoLocalesEnabled = true
        }
        release {
            signingConfig = signingConfigs.getByName("release")
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    sourceSets {
        getByName("main") {
            java.srcDir(layout.buildDirectory.dir("generated/source/uniffi"))
            jniLibs.srcDir(layout.buildDirectory.dir("generated/jniLibs"))
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        buildConfig = true
        compose = true
    }
}

androidComponents {
    onVariants(selector().withBuildType("debug")) { variant ->
        variant.applicationId.set("org.rstorrent.bootstrap")
    }
}

val rustlsPlatformVerifierVersion = run {
    val metadata = providers.exec {
        workingDir = rootProject.projectDir
        commandLine(
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            "aarch64-linux-android",
            "--manifest-path",
            "../../crates/rstorrent-android/Cargo.toml",
        )
    }.standardOutput.asText.get()
    @Suppress("UNCHECKED_CAST")
    val packages =
        (JsonSlurper().parseText(metadata) as Map<String, Any?>)["packages"] as List<Map<String, Any?>>
    packages.single { it["name"] == "rustls-platform-verifier-android" }.getValue("version") as String
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation("androidx.annotation:annotation:1.8.0")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.5")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.5")
    implementation("androidx.lifecycle:lifecycle-process:2.8.5")
    implementation("androidx.media3:media3-exoplayer:1.9.2")
    implementation("androidx.media3:media3-ui:1.9.2")
    implementation("androidx.navigation:navigation-compose:2.8.0")
    implementation(platform("androidx.compose:compose-bom:2024.09.00"))
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("rustls:rustls-platform-verifier:$rustlsPlatformVerifierVersion@aar")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation(platform("androidx.compose:compose-bom:2024.09.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:core-ktx:1.6.1")
    androidTestImplementation("androidx.test:rules:1.6.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
}

// AGP otherwise permits an unsigned release when no storeFile is configured.
val requireReleaseSigning by tasks.registering {
    doLast {
        for (name in listOf("UPLOAD_KEYSTORE_PATH", "UPLOAD_KEYSTORE_PASSWORD", "UPLOAD_KEY_ALIAS", "UPLOAD_KEY_PASSWORD")) {
            require(!providers.environmentVariable(name).orNull.isNullOrBlank()) {
                "Release builds require $name; unsigned/debug-key fallback is disabled"
            }
        }
    }
}
tasks.configureEach {
    if (name == "preReleaseBuild") {
        dependsOn(requireReleaseSigning)
    }
}
