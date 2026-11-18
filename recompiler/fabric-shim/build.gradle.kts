// Recompiles one unchanged Fabric mod JAR (-PmodJar=...) together with this shim to C.
plugins {
    java
    id("org.teavm") version "0.16.0"
}
repositories {
    mavenCentral()
    maven("https://libraries.minecraft.net") // Brigadier (MIT)
}

val modJar = file(providers.gradleProperty("modJar").get())
val entrypointsDir = layout.buildDirectory.dir("generated/entrypoints")

// TeaVM has no reflection, so the fabric.mod.json "main" entrypoints become plain constructor calls.
val generateEntrypoints = tasks.register("generateEntrypoints") {
    inputs.file(modJar)
    outputs.dir(entrypointsDir)
    doLast {
        val json = zipTree(modJar).matching { include("fabric.mod.json") }.singleFile.readText()
        val meta = groovy.json.JsonSlurper().parseText(json) as Map<*, *>
        val main = ((meta["entrypoints"] as? Map<*, *>)?.get("main") as? List<*>).orEmpty()
        val calls = main.joinToString("\n") { entry ->
            val target = (if (entry is Map<*, *>) entry["value"] else entry) as String
            // shortcut: only class entrypoints; "Class::field" and adapters come when a mod in the corpus needs them.
            require("::" !in target) { "unsupported entrypoint $target" }
            "        new $target().onInitialize();"
        }
        val out = entrypointsDir.get().file("moddedmc/rt/Entrypoints.java").asFile
        out.parentFile.mkdirs()
        out.writeText("package moddedmc.rt;\n\nfinal class Entrypoints {\n    static void init() {\n$calls\n    }\n}\n")
        file(layout.buildDirectory.file("mod-id.txt")).writeText(meta["id"] as String)
    }
}
sourceSets.main { java.srcDir(entrypointsDir) }
tasks.compileJava {
    dependsOn(generateEntrypoints)
    options.release = 21
}

dependencies {
    implementation(teavm.libs.interop)
    implementation("com.mojang:brigadier:1.3.11")
    implementation(files(modJar))
}
teavm {
    c {
        mainClass = "moddedmc.rt.ModRuntime"
    }
}
