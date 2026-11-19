package dev.moddedmc.companion;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class ProtocolTest {
    @Test
    void advertisesBigEndianVersionAndFlags() {
        assertArrayEquals(new byte[] {0, 1, 0, 0, 0, 0}, Protocol.hello());
    }

    @Test
    void rejectsTruncatedAndUnsupportedAnswers() {
        assertNull(Protocol.answer(new byte[] {0, 1}));
        assertNull(Protocol.answer(new byte[] {0, 2, 0, 0, 0, 0}));
        assertEquals(0, Protocol.answer(new byte[] {0, 1, 0, 0, 0, 0}).flags());
    }
}
