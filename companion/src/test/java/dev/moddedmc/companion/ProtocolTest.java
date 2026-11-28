package dev.moddedmc.companion;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class ProtocolTest {
    @Test
    void rigBoneUsesBoundedNamesAndFiniteBigEndianCoordinates() {
        var data = java.nio.ByteBuffer.wrap(Protocol.rigBone(42, "hand", 1, 2, 3));
        assertEquals(1, data.get());
        assertEquals(6, data.get());
        assertEquals(42, data.getInt());
        assertEquals(4, data.get());
        byte[] name = new byte[4];
        data.get(name);
        assertArrayEquals(new byte[] {'h', 'a', 'n', 'd'}, name);
        assertEquals(1.0, data.getDouble());
        assertEquals(2.0, data.getDouble());
        assertEquals(3.0, data.getDouble());
        assertFalse(data.hasRemaining());
        assertNull(Protocol.rigBone(42, "bad bone", 1, 2, 3));
        assertNull(Protocol.rigBone(42, "x".repeat(65), 1, 2, 3));
        assertNull(Protocol.rigBone(42, "hand", Double.NaN, 2, 3));
    }

    @Test
    void advertisesBigEndianVersionAndFlags() {
        assertArrayEquals(new byte[] {0, 1, 0, 0, 0, 3}, Protocol.hello());
    }

    @Test
    void rejectsTruncatedAndUnsupportedAnswers() {
        assertNull(Protocol.answer(new byte[] {0, 1}));
        assertNull(Protocol.answer(new byte[] {0, 2, 0, 0, 0, 0}));
        assertNull(Protocol.answer(new byte[] {0, 1, 0, 0, 0, 1}));
        assertEquals(0, Protocol.answer(new byte[] {0, 1, 0, 0, 0, 0}).flags());
        var actionsOnly = Protocol.answer(new byte[] {0, 1, 0, 0, 0, 2});
        assertEquals(2, actionsOnly.flags());
        assertNull(actionsOnly.offer());
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
