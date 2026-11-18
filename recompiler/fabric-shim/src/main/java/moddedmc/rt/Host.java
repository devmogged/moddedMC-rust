package moddedmc.rt;

import java.nio.charset.StandardCharsets;

import org.teavm.interop.Address;
import org.teavm.interop.Import;

/**
 * Calls into the Rust plugin that hosts this runtime (recompiler/fabric-host). Strings cross as
 * (pointer, length) of UTF-8 in the shared linear memory. Player handles are only valid during the
 * export call that produced them.
 */
public final class Host {
    public static final int TEXT_PLAYER_NAME = 1;
    public static final int TEXT_PLAYER_DIMENSION = 2;
    public static final int TEXT_CONFIG_DIR = 3;
    public static final int TEXT_WORLD_DIMENSION = 4;

    public static final int NUM_X = 0, NUM_Y = 1, NUM_Z = 2, NUM_YAW = 3, NUM_PITCH = 4;

    private Host() {}

    @Import(module = "mmc", name = "mmc_never")
    static native int never();

    @Import(module = "mmc", name = "mmc_register_command")
    static native void registerCommand(Address name, int length);

    /** Writes up to {@code capacity} bytes and returns the full length, or -1 if there is no value. */
    @Import(module = "mmc", name = "mmc_text")
    static native int text(int op, int handle, Address buffer, int capacity);

    @Import(module = "mmc", name = "mmc_player_num")
    public static native double playerNum(int player, int field);

    @Import(module = "mmc", name = "mmc_send_message")
    static native void sendMessage(int player, Address utf8, int length);

    @Import(module = "mmc", name = "mmc_teleport")
    static native void teleport(int player, Address dimension, int length, double x, double y, double z,
            float yaw, float pitch);

    @Import(module = "mmc", name = "mmc_tick_count")
    public static native int tickCount();

    @Import(module = "mmc", name = "mmc_world_count")
    public static native int worldCount();

    public static String text(int op, int handle) {
        byte[] buffer = new byte[256];
        int length = text(op, handle, Address.ofData(buffer), buffer.length);
        if (length < 0) {
            return null;
        }
        if (length > buffer.length) {
            buffer = new byte[length];
            text(op, handle, Address.ofData(buffer), buffer.length);
        }
        return new String(buffer, 0, length, StandardCharsets.UTF_8);
    }

    public static void sendMessage(int player, String text) {
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        sendMessage(player, Address.ofData(bytes), bytes.length);
    }

    public static void teleport(int player, String dimension, double x, double y, double z, float yaw, float pitch) {
        byte[] bytes = dimension.getBytes(StandardCharsets.UTF_8);
        teleport(player, Address.ofData(bytes), bytes.length, x, y, z, yaw, pitch);
    }

    static void registerCommand(String name) {
        byte[] bytes = name.getBytes(StandardCharsets.UTF_8);
        registerCommand(Address.ofData(bytes), bytes.length);
    }

    static String read(Address utf8, int length) {
        byte[] bytes = new byte[length];
        for (int i = 0; i < length; i++) {
            bytes[i] = utf8.add(i).getByte();
        }
        return new String(bytes, StandardCharsets.UTF_8);
    }
}
