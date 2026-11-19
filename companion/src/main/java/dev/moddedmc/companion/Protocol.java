package dev.moddedmc.companion;

import java.nio.ByteBuffer;

final class Protocol {
    static final short VERSION = 1;
    static final int FLAGS = 0;
    static final int HEADER_SIZE = Short.BYTES + Integer.BYTES;

    static byte[] hello() {
        return ByteBuffer.allocate(HEADER_SIZE).putShort(VERSION).putInt(FLAGS).array();
    }

    static Answer answer(byte[] bytes) {
        if (bytes.length != HEADER_SIZE) return null;
        var data = ByteBuffer.wrap(bytes);
        if (data.getShort() != VERSION) return null;
        return new Answer(data.getInt() & FLAGS);
    }

    record Answer(int flags) {}
}
