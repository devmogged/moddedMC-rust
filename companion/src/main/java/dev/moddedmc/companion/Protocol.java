package dev.moddedmc.companion;

import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.HexFormat;

final class Protocol {
    static final short VERSION = 1;
    static final int HASHED_ASSETS = 1;
    static final int PLAYER_ACTIONS = 2;
    static final int FLAGS = HASHED_ASSETS | PLAYER_ACTIONS;
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

    static byte[] rigBone(int entity, String bone, double x, double y, double z) {
        if (!bone.matches("[A-Za-z0-9_.:-]{1,64}")
                || !Double.isFinite(x) || !Double.isFinite(y) || !Double.isFinite(z)) return null;
        byte[] name = bone.getBytes(StandardCharsets.UTF_8);
        return ByteBuffer.allocate(7 + name.length + 24)
                .put((byte) VERSION).put((byte) 6).putInt(entity).put((byte) name.length)
                .put(name).putDouble(x).putDouble(y).putDouble(z).array();
    }

    record Answer(int flags, Offer offer) {}
    record Offer(String manifest, String zipSha1) {}
}
