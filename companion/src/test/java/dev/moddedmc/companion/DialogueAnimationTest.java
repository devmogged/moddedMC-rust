package dev.moddedmc.companion;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class DialogueAnimationTest {
    @Test
    void typewriterDoesNotSplitUnicode() {
        String text = "A\uD83D\uDE00B";
        assertEquals(0, DialogueAnimation.visibleCount(text, 120, 120, 36));
        assertEquals(1, DialogueAnimation.visibleCount(text, 156, 120, 36));
        assertEquals("A\uD83D\uDE00", DialogueAnimation.prefix(text,
                DialogueAnimation.visibleCount(text, 192, 120, 36)));
        assertEquals(3, DialogueAnimation.visibleCount(text, 10000, 120, 36));
    }

    @Test
    void voiceSkipsWhitespacePunctuationAndAlternateCharacters() {
        assertTrue(DialogueAnimation.voiced('a', 0));
        assertFalse(DialogueAnimation.voiced('a', 1));
        assertFalse(DialogueAnimation.voiced('!', 2));
        assertFalse(DialogueAnimation.voiced(' ', 4));
        assertEquals(.5f, DialogueAnimation.easeInOut(.5f));
        assertEquals(1f, DialogueAnimation.easeOut(1f));
    }

    @Test
    void waveSmoothingAdvancesAndDecaysByGameTicks() {
        assertEquals(.12f, WaveOverlay.advance(0, 1, .12f, 1), .00001f);
        assertEquals(.2256f, WaveOverlay.advance(0, 1, .12f, 2), .00001f);
        assertEquals(.88f, WaveOverlay.advance(1, 0, .12f, 1), .00001f);
        assertEquals(.5f, WaveOverlay.advance(.5f, 0, .12f, 0));
    }
}
