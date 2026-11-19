package dev.moddedmc.companion;

import java.nio.ByteBuffer;
import java.util.HexFormat;

final class Protocol {
    static final short VERSION = 1;
    static final int HASHED_ASSETS = 1;
    static final int FLAGS = HASHED_ASSETS;
    static final int HEADER_SIZE = Short.BYTES + Integer.BYTES;

    static byte[] hello() {
        return ByteBuffer.allocate(HEADER_SIZE).putShort(VERSION).putInt(FLAGS).array();
    }

    static Answer answer(byte[] bytes) {
        if (bytes.length < HEADER_SIZE) return null;
        var data = ByteBuffer.wrap(bytes);
        if (data.getShort() != VERSION) return null;
        int flags = data.getInt() & FLAGS;
        if ((flags & HASHED_ASSETS) == 0) return new Answer(flags, null);
        if (data.remaining() < 32 + 20) return null;
        byte[] manifest = new byte[32];
        byte[] zip = new byte[20];
        data.get(manifest).get(zip);
        var hex = HexFormat.of();
        return new Answer(flags, new Offer(hex.formatHex(manifest), hex.formatHex(zip)));
    }

    record Answer(int flags, Offer offer) {}
    record Offer(String manifest, String zipSha1) {}
}
