package probe;

import org.teavm.interop.Export;
import org.teavm.interop.Import;

public class Probe {
    @Import(module = "host", name = "mmc_log")
    static native void log(int value);

    @Export(name = "add")
    public static int add(int a, int b) {
        log(a * 1000 + b);
        return a + b;
    }

    public static void main(String[] args) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 3; i++) sb.append(i);
        log(sb.toString().length());
    }
}
