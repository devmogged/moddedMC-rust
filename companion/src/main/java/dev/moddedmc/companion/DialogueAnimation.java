package dev.moddedmc.companion;

/** Client clock for a pack dialogue, independent of server tick and packet delay. */
final class DialogueAnimation {
    static int visibleCount(String text, long elapsed, int delay, int interval) {
        return (int)Math.clamp((elapsed - delay) / interval, 0, text.codePointCount(0, text.length()));
    }

    static String prefix(String text, int count) {
        return text.substring(0, text.offsetByCodePoints(0, count));
    }

    static boolean voiced(int codePoint, int index) {
        return index % 2 == 0 && !Character.isWhitespace(codePoint)
                && ".,!?;:()[]{}\"'".indexOf(codePoint) < 0;
    }

    static float easeOut(float t) {
        float f = 1 - t;
        return 1 - f * f * f;
    }

    static float easeInOut(float t) {
        if (t < .5f) return 4 * t * t * t;
        float f = 2 - 2 * t;
        return 1 - f * f * f / 2;
    }
}
