# R8 rules for the release build.
#
# `isMinifyEnabled = true` referenced this file for a long time while the file
# did not exist, so release had never been built — which is why the shipped APK
# was a 43 MB debug build. Every rule below is here for a reason that is named;
# nothing is a blanket keep copied from a template.
#
# Verified by building the release APK and driving all 26 screens against the
# live agent with scripts/walk-app.py. That matters more than the rules
# themselves: R8 failures are runtime failures, and they are invisible to a
# build that only has to compile.

# ── kotlinx.serialization ─────────────────────────────────────────────────────
#
# The plugin generates a `Companion.serializer()` per @Serializable class and
# looks it up reflectively. R8 sees no call site, removes it, and the app dies
# at the first decode with "Serializer for class X is not found" — at runtime,
# on whichever screen decodes first.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.**

-keepclassmembers class kotlinx.serialization.json.** {
    *** Companion;
}
-keepclasseswithmembers class kotlinx.serialization.json.** {
    kotlinx.serialization.KSerializer serializer(...);
}

-if @kotlinx.serialization.Serializable class **
-keepclassmembers class <1> {
    static <1>$Companion Companion;
}
-if @kotlinx.serialization.Serializable class ** {
    static **$* *;
}
-keepclassmembers class <2>$<3> {
    kotlinx.serialization.KSerializer serializer(...);
}
-if @kotlinx.serialization.Serializable class **
-keepclassmembers class <1>$Companion {
    kotlinx.serialization.KSerializer serializer(...);
}

# The app's own models are decoded by name through reified helpers in
# AgentClient, so their fields must keep the names the agent sends.
-keepclassmembers @kotlinx.serialization.Serializable class com.openu60.** {
    <fields>;
    **$* *;
}

# ── OkHttp ────────────────────────────────────────────────────────────────────
#
# OkHttp ships its own consumer rules; these only silence warnings for optional
# compile-time dependencies it references but does not need at runtime.
-dontwarn okhttp3.internal.platform.**
-dontwarn org.conscrypt.**
-dontwarn org.bouncycastle.**
-dontwarn org.openjsse.**

# ── Compose, Hilt, ML Kit, CameraX, Vico ──────────────────────────────────────
#
# All four ship consumer rules in their AARs, so nothing is needed here. Said
# explicitly because the temptation with an R8 failure is to add a broad
# `-keep class **` and move on, which turns minification off in all but name.

# ── Crash readability ─────────────────────────────────────────────────────────
#
# Without this a stack trace from a release build names obfuscated classes and
# no line numbers, which makes a bug report from a real device useless. The
# mapping file lands in app/build/outputs/mapping/release/.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
