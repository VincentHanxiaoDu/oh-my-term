plugins {
    id("com.android.application")
    kotlin("android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.ohmyterm.omt"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.ohmyterm.omt"
        // 26, because a foreground service holding a socket is the whole reason
        // this app exists rather than the PWA, and the service APIs below that
        // are different enough to be a second implementation.
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    // Both compilers on the same target. Left mismatched, the Java half
    // silently builds for 8 and the Kotlin half for 17, and the build fails
    // with a message about neither.
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions { jvmTarget = "17" }

    buildFeatures { compose = true }
}

dependencies {
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation(platform("androidx.compose:compose-bom:2024.09.03"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    testImplementation("junit:junit:4.13.2")
}
