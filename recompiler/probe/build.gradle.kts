plugins {
    java
    id("org.teavm") version "0.16.0"
}
repositories { mavenCentral() }
dependencies { implementation(teavm.libs.interop) }
teavm {
    wasmGC {
        mainClass = "probe.Probe"
    }
    c {
        mainClass = "probe.Probe"
    }
}
