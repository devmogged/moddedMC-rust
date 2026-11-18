package probe;

import org.teavm.interop.Address;
import org.teavm.interop.Export;
import org.teavm.interop.Import;

public class Probe {
    static int calls;

    @Import(module = "host", name = "mmc_log")
    static native void log(int value);

    // A string crosses as (pointer, length) of UTF-8 bytes in TeaVM's heap.
    @Import(module = "host", name = "mmc_print")
    static native void print(Address utf8, int length);

    @Import(module = "host", name = "mmc_never")
    static native int never();

    static void say(String text) {
        byte[] bytes = text.getBytes(java.nio.charset.StandardCharsets.UTF_8);
        print(Address.ofData(bytes), bytes.length);
    }

    @Export(name = "probe_add")
    public static int add(int a, int b) {
        calls++;
        say("add " + a + "+" + b + " (call " + calls + ")");
        return a + b;
    }

    public static void main(String[] args) {
        // Keeps the exports reachable for TeaVM's dead-code elimination.
        if (never() == 1) {
            add(0, 0);
        }
        say("runtime up");
    }
}
