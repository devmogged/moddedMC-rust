package dev.moddedmc.companion;

import java.util.Arrays;
import java.util.function.IntConsumer;

/** The release samples movement key edges with a strict 300 ms window. */
final class MountedInput {
    private static final long DOUBLE_TAP_NANOS = 300_000_000L;
    private final boolean[] down = new boolean[2], directionDown = new boolean[4], tapped = new boolean[4];
    private final int[] ticks = new int[2];
    private final long[] lastTap = new long[4];

    void mouse(int index, boolean held, boolean clicked, IntConsumer emit) {
        if (!down[index] && (held || clicked)) {
            emit.accept(1);
            ticks[index] = 0;
            if (!held) emit.accept(4);
        }
        if (held && ++ticks[index] % 4 == 0) emit.accept(2);
        if (down[index] && !held) emit.accept(4);
        down[index] = held;
    }

    boolean tap(int index, boolean held, long now) {
        boolean edge = held && !directionDown[index];
        directionDown[index] = held;
        if (!edge) return false;
        boolean twice = tapped[index] && now - lastTap[index] < DOUBLE_TAP_NANOS;
        tapped[index] = true;
        lastTap[index] = now;
        return twice;
    }

    void resetDirections() {
        Arrays.fill(directionDown, false);
        Arrays.fill(tapped, false);
    }

    void reset() {
        Arrays.fill(down, false);
        Arrays.fill(ticks, 0);
        resetDirections();
    }
}
