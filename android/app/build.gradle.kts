import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.synchro.app"
    compileSdk = 36
    defaultConfig {
        applicationId = "com.synchro.app"
        minSdk = 29
        targetSdk = 35
        versionCode = 7
        versionName = "0.6.0"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation("androidx.work:work-runtime:2.10.2")
    implementation("androidx.documentfile:documentfile:1.0.1")
}

// Cross-platform AES-GCM backup envelope golden-vector test.
dependencies {
    testImplementation("junit:junit:4.13.2")
}
