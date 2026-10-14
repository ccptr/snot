// AGP 9 applies the Kotlin plugin itself; only the Compose compiler plugin is
// listed here.
plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

val minApi = 26

/**
 * The app's version is the Rust workspace's, so the two cannot drift.
 * versionCode orders like the version, so every release upgrades the last.
 */
val appVersion = Regex("""(?ms)^\[workspace\.package\].*?^version = "([^"]+)"""")
    .find(rootProject.projectDir.parentFile.resolve("Cargo.toml").readText())
    ?.groupValues?.get(1) ?: error("No [workspace.package] version in Cargo.toml")
val appVersionCode = appVersion.substringBefore('-').split('.').map(String::toInt)
    .let { (major, minor, patch) -> major * 1_000_000 + minor * 1_000 + patch }

/**
 * ABIs to build snot-ffi for, as `abi to Rust target`; each gets its own APK.
 * Override with `-PrustAbis=arm64-v8a` to build fewer, or add `x86` for an
 * old emulator.
 */
val rustTargets = mapOf(
    "arm64-v8a" to "aarch64-linux-android",
    "x86" to "i686-linux-android",
    "x86_64" to "x86_64-linux-android",
)
val rustAbis = ((findProperty("rustAbis") as String?) ?: "arm64-v8a,x86_64")
    .split(',').map { it.trim() }.filter { it.isNotEmpty() }

/**
 * Cargo profile for snot-ffi. Optimised by default, even in debug builds, so
 * ink keeps up; CI type-checks and tests with `-PrustProfile=dev`.
 */
val rustProfile = (findProperty("rustProfile") as String?) ?: "release"
val rustProfileDir = if (rustProfile == "dev") "debug" else rustProfile

val rustLibs = layout.buildDirectory.dir("rustJniLibs")
val bindingsDir = layout.buildDirectory.dir("generated/uniffi")

android {
    namespace = "app.snot"
    compileSdk = 37
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "dev.ccptr.Snot"
        minSdk = minApi
        targetSdk = 37
        versionCode = appVersionCode
        versionName = appVersion
    }

    // One APK per ABI, each with only its own native libraries. This also
    // leaves out JNA's dispatch libraries for ABIs snot-ffi is not built for.
    splits {
        abi {
            isEnable = true
            reset()
            include(*rustAbis.toTypedArray())
            isUniversalApk = false
        }
    }

    // The release key comes from the environment so that it only ever lives in
    // CI secrets. Without SNOT_KEYSTORE, assembleRelease gives an unsigned APK.
    val keystore = providers.environmentVariable("SNOT_KEYSTORE").orNull
    if (keystore != null) {
        signingConfigs.create("release") {
            storeFile = file(keystore)
            keyAlias = providers.environmentVariable("SNOT_KEY_ALIAS").get()
            // A PKCS12 keystore, keytool's default, has one password for both.
            storePassword = providers.environmentVariable("SNOT_KEYSTORE_PASSWORD").get()
            keyPassword = storePassword
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets["main"].jniLibs.directories.add(rustLibs.get().asFile.path)
    sourceSets["main"].kotlin.directories.add(bindingsDir.get().asFile.path)
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

// ---- snot-ffi ----------------------------------------------------------------
//
// The Rust core is compiled once per ABI with the NDK's clang as the linker,
// optimised unless rustProfile says otherwise, and its Kotlin bindings are generated from the library
// itself so they can never drift from it.

val workspaceDir = rootProject.projectDir.parentFile
val ndkBin = androidComponents.sdkComponents.ndkDirectory.map {
    val host = when {
        org.gradle.internal.os.OperatingSystem.current().isMacOsX -> "darwin-x86_64"
        org.gradle.internal.os.OperatingSystem.current().isWindows -> "windows-x86_64"
        else -> "linux-x86_64"
    }
    it.asFile.resolve("toolchains/llvm/prebuilt/$host/bin")
}

fun taskSuffix(abi: String) = abi.replace("-", "").replaceFirstChar(Char::uppercase)

val cargoTasks = rustAbis.map { abi ->
    val target = rustTargets[abi] ?: error("Unknown ABI '$abi'; known: ${rustTargets.keys}")
    tasks.register<Exec>("cargoBuild${taskSuffix(abi)}") {
        group = "build"
        description = "Builds snot-ffi for $abi"
        workingDir = workspaceDir
        // cargo does its own up-to-date checking.
        outputs.upToDateWhen { false }

        val envName = target.uppercase().replace('-', '_')
        doFirst {
            val bin = ndkBin.get()
            val suffix = if (org.gradle.internal.os.OperatingSystem.current().isWindows) ".cmd" else ""
            val clang = bin.resolve("$target$minApi-clang$suffix").path
            environment("CARGO_TARGET_${envName}_LINKER", clang)
            environment("CC_${target.replace('-', '_')}", clang)
            environment("AR_${target.replace('-', '_')}", bin.resolve("llvm-ar").path)
            // Android 15 devices may use 16 KB pages; NDK r27 still links for
            // 4 KB unless told otherwise, and such a library fails to load.
            environment("CARGO_TARGET_${envName}_RUSTFLAGS", "-C link-arg=-Wl,-z,max-page-size=16384")
        }
        commandLine("cargo", "build", "--profile", rustProfile, "-p", "snot-ffi", "--target", target)
        doLast {
            copy {
                from(workspaceDir.resolve("target/$target/$rustProfileDir/libsnot_ffi.so"))
                into(rustLibs.get().dir(abi))
            }
        }
    }
}

val uniffiBindings = tasks.register<Exec>("uniffiBindings") {
    group = "build"
    description = "Generates the Kotlin bindings for snot-ffi"
    val abi = rustAbis.first()
    dependsOn("cargoBuild${taskSuffix(abi)}")
    workingDir = workspaceDir
    outputs.dir(bindingsDir)
    outputs.upToDateWhen { false }
    doFirst { delete(bindingsDir) }
    commandLine(
        "cargo", "run", "-q", "-p", "uniffi-bindgen", "--",
        "generate", "--no-format",
        "--library", rustLibs.get().file("$abi/libsnot_ffi.so").asFile.path,
        "--language", "kotlin",
        "--out-dir", bindingsDir.get().asFile.path,
    )
}

tasks.named("preBuild") { dependsOn(cargoTasks, uniffiBindings) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.graphics.core)

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.foundation)
    implementation(libs.androidx.compose.material3)

    // UniFFI's Kotlin bindings call into the library through JNA.
    implementation("${libs.jna.get()}@aar")
}
