plugins {
    id("com.android.application")
}

android {
    namespace = "org.rstorrent.storageprobe"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.rstorrent.storageprobe"
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = "0.1"

        ndk {
            abiFilters += "x86_64"
        }
    }

    buildTypes {
        debug {
            isDebuggable = true
        }
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
