import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
    id("com.google.dagger.hilt.android")
    id("com.google.devtools.ksp")
}

// Release signing, read from a file that is not in the repository.
//
// There is no Play Store account in the picture: a release APK needs a signing
// key and nothing else. The key is the app's identity, though — anything signed
// with it can replace an installed copy, and losing it means no installed copy
// can be updated in place again — so it lives in keystore.properties beside the
// .jks, both gitignored.
//
// Without that file the release build still runs and falls back to the debug
// key, which installs and runs fine for sideloading. That is deliberate: a
// checkout on another machine should build, not fail on a missing secret.
val keystoreProperties = Properties().apply {
    val file = rootProject.file("keystore.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}

android {
    namespace = "com.openu60"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.openu60"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "1.0.0"
    }

    signingConfigs {
        if (keystoreProperties.getProperty("storeFile") != null) {
            create("release") {
                storeFile = rootProject.file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            // Shrink resources too. Most of what R8 leaves behind in this app
            // is drawables and strings from libraries no screen uses.
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    // One APK per ABI instead of one carrying all four.
    //
    // ML Kit's barcode model is bundled on purpose (see the dependency below),
    // and it is a native library, so a universal APK ships four copies of it —
    // about 20 MB of the 43 MB total for three architectures the target device
    // will never run. Every phone this is installed on is arm64-v8a.
    //
    // `isUniversalApk` stays on because the emulator used for testing is
    // x86_64, and a build you cannot run on the machine that built it is a
    // build nobody checks.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    testOptions {
        unitTests {
            // The parsers are pure Kotlin but live in the same module as the
            // Compose screens, so a test that touches an android.* class would
            // otherwise die on "not mocked" instead of failing on its own terms.
            isReturnDefaultValues = true
        }
    }
}

dependencies {
    // Compose BOM
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    debugImplementation("androidx.compose.ui:ui-tooling")

    // Core
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.activity:activity-compose:1.9.3")

    // Navigation
    implementation("androidx.navigation:navigation-compose:2.8.5")

    // Hilt
    implementation("com.google.dagger:hilt-android:2.54")
    ksp("com.google.dagger:hilt-android-compiler:2.54")
    implementation("androidx.hilt:hilt-navigation-compose:1.2.0")

    // Network
    implementation("com.squareup.okhttp3:okhttp:4.12.0")

    // Serialization
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")

    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    // Encrypted SharedPreferences
    implementation("androidx.security:security-crypto:1.1.0-alpha06")

    // Charts (Vico)
    implementation("com.patrykandpatrick.vico:compose-m3:2.0.1")

    // QR scanning for eSIM activation codes.
    //
    // ML Kit's *bundled* barcode model is used deliberately: the unbundled one
    // downloads the model through Play Services on first use, and the phone
    // running this is very often joined to the router's Wi-Fi, which has no
    // internet — exactly the situation where you need to scan a code. Bundling
    // costs a few MB of APK and works offline.
    implementation("com.google.mlkit:barcode-scanning:17.3.0")
    implementation("androidx.camera:camera-camera2:1.4.1")
    implementation("androidx.camera:camera-lifecycle:1.4.1")
    implementation("androidx.camera:camera-view:1.4.1")

    // Pull-to-refresh
    // (included in material3)

    // Parser tests. These run on the JVM, not a device: everything they cover
    // is pure Kotlin over the maps AgentClient produces, which is exactly where
    // the blank-field bugs have lived.
    testImplementation("junit:junit:4.13.2")
}
