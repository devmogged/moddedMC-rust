package dev.moddedmc.companion;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class ProtocolTest {
    @Test
    void advertisesBigEndianVersionAndFlags() {
        assertArrayEquals(new byte[] {0, 1, 0, 0, 0, 1}, Protocol.hello());
    }

    @Test
    void rejectsTruncatedAndUnsupportedAnswers() {
        assertNull(Protocol.answer(new byte[] {0, 1}));
        assertNull(Protocol.answer(new byte[] {0, 2, 0, 0, 0, 0}));
        assertNull(Protocol.answer(new byte[] {0, 1, 0, 0, 0, 1}));
        assertEquals(0, Protocol.answer(new byte[] {0, 1, 0, 0, 0, 0}).flags());
    }

    @Test
    void bindsAssetsToTheVanillaZipHash() {
        var bytes = new byte[58];
        bytes[1] = 1;
        bytes[5] = 1;
        bytes[6] = (byte) 0xab;
        bytes[38] = (byte) 0xcd;
        var answer = Protocol.answer(bytes);
        assertEquals(1, answer.flags());
        assertEquals("ab" + "00".repeat(31), answer.offer().manifest());
        assertEquals("cd" + "00".repeat(19), answer.offer().zipSha1());
    }
}
