package dev.moddedmc.companion;

import com.mojang.blaze3d.platform.InputConstants;
import dev.moddedmc.companion.mixin.OptionsAccessor;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientConfigurationConnectionEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

public final class PlayerActions {
    private static final int VERSION = 1, PRESSED = 1, HELD = 2, RELEASED = 4;
    private static final int MAX_ACTIONS = 128, MAX_PACKET = 42000;
    private static volatile boolean enabled;
    private static final Map<String, KeyMapping> known = new HashMap<>();
    private static final Map<String, KeyMapping.Category> categories = new HashMap<>();
    private static final Map<String, Binding> active = new LinkedHashMap<>();
    private static final Map<String, Definition> mounted = new HashMap<>();
    private static final String ATTACK = "moddedmc:mounted_attack", USE = "moddedmc:mounted_use";
    private static final String[] DIRECTIONS = {"forward", "back", "left", "right"};
    private static final String BUILTIN_KEY = "moddedmc:mounted";
    private static final MountedInput mountedInput = new MountedInput();
    private static int queriedVehicle;
    private static boolean confirmedVehicle;
    private static boolean mountReplyReceived;
    private static int queryTicks;
    private static final Properties saved = new Properties();
    private static final java.nio.file.Path SAVE = FabricLoader.getInstance().getConfigDir()
            .resolve("moddedmc-player-actions.properties");

    static void negotiated(boolean value) { enabled = value; }

    static void register() {
        try (var input = Files.newInputStream(SAVE)) { saved.load(input); }
        catch (IOException ignored) {}
        PayloadTypeRegistry.clientboundPlay().register(Packet.TYPE, Packet.CODEC);
        PayloadTypeRegistry.serverboundPlay().register(Packet.TYPE, Packet.CODEC);
        ClientConfigurationConnectionEvents.INIT.register((listener, client) -> enabled = false);
        ClientPlayConnectionEvents.JOIN.register((listener, sender, client) -> {
            active.clear();
            resetMounted();
            if (enabled) listener.send(new ServerboundCustomPayloadPacket(new Packet(new byte[] {VERSION, 0})));
        });
        ClientPlayConnectionEvents.DISCONNECT.register((listener, client) -> {
            persist();
            active.clear();
            resetMounted();
            enabled = false;
        });
        ClientPlayNetworking.registerGlobalReceiver(Packet.TYPE, (packet, context) -> {
            byte[] bytes = packet.bytes();
            if (bytes.length == 7 && bytes[0] == VERSION && bytes[1] == 4) {
                int id = ByteBuffer.wrap(bytes, 2, 4).getInt();
                boolean allowed = bytes[6] == 1;
                context.client().execute(() -> {
                    if (enabled && queriedVehicle == id && vehicleId(context.client()) == id) {
                        confirmedVehicle = allowed;
                        mountReplyReceived = true;
                    }
                });
                return;
            }
            var actions = decode(packet.bytes());
            if (actions != null) context.client().execute(() -> {
                if (enabled && context.client().getConnection() != null) install(context.client(), actions);
            });
        });
        ClientTickEvents.START_CLIENT_TICK.register(PlayerActions::tickMounted);
        ClientTickEvents.END_CLIENT_TICK.register(PlayerActions::tick);
    }

    private record Definition(String id, String translation, String key, int phases) {}

    private static List<Definition> decode(byte[] bytes) {
        try {
            var data = ByteBuffer.wrap(bytes);
            if (data.get() != VERSION || data.get() != 1) return null;
            int count = Short.toUnsignedInt(data.getShort());
            if (count > MAX_ACTIONS) return null;
            var result = new ArrayList<Definition>();
            var ids = new java.util.HashSet<String>();
            for (int i = 0; i < count; i++) {
                String id = field(data), translation = field(data), key = field(data);
                int phases = Byte.toUnsignedInt(data.get());
                if (!id.matches("[a-z0-9_.-]+:[a-z0-9_./-]+") || !ids.add(id)
                        || id.length() > 128 || translation.isEmpty() || translation.length() > 128
                        || key.isEmpty() || key.length() > 64 || phases == 0 || (phases & ~7) != 0) return null;
                result.add(new Definition(id, translation, key, phases));
            }
            return data.hasRemaining() ? null : result;
        } catch (RuntimeException invalid) { return null; }
    }

    private static String field(ByteBuffer data) {
        byte[] bytes = new byte[Byte.toUnsignedInt(data.get())];
        data.get(bytes);
        return new String(bytes, StandardCharsets.UTF_8);
    }

    private static void install(Minecraft client, List<Definition> definitions) {
        active.clear();
        resetMounted();
        var mappings = new ArrayList<>(Arrays.asList(client.options.keyMappings));
        for (var action : definitions) {
            if (action.key.equals(BUILTIN_KEY)) {
                if (action.id.equals(ATTACK) || action.id.equals(USE)
                        || Arrays.stream(DIRECTIONS).anyMatch(dir -> action.id.equals("moddedmc:double_tap/" + dir))) {
                    mounted.put(action.id, action);
                }
                continue;
            }
            boolean ctrl = action.key.startsWith("ctrl+");
            String keyName = ctrl ? action.key.substring(5) : action.key;
            var mapping = known.get(action.id);
            if (mapping == null) {
                if (known.size() >= 512 || KeyMapping.get(action.translation) != null) continue;
                String namespace = action.id.substring(0, action.id.indexOf(':'));
                var category = categories.computeIfAbsent(namespace, name ->
                        KeyMapping.Category.register(Identifier.fromNamespaceAndPath("moddedmc", "actions." + name)));
                try {
                    var key = keyName.equals("key.keyboard.unknown") ? InputConstants.UNKNOWN : InputConstants.getKey(keyName);
                    mapping = new KeyMapping(action.translation, key.getType(), key.getValue(), category);
                    String previous = saved.getProperty(action.id);
                    if (previous != null) {
                        try { mapping.setKey(InputConstants.getKey(previous)); }
                        catch (IllegalArgumentException ignored) {}
                    }
                    known.put(action.id, mapping);
                    mappings.add(mapping);
                } catch (IllegalArgumentException invalid) { continue; }
            }
            active.put(action.id, new Binding(action, mapping, ctrl));
        }
        ((OptionsAccessor) client.options).moddedmc$setKeyMappings(mappings.toArray(KeyMapping[]::new));
        KeyMapping.resetMapping();
    }

    private static void resetMounted() {
        mounted.clear();
        queriedVehicle = 0;
        confirmedVehicle = false;
        mountReplyReceived = false;
        queryTicks = 0;
        mountedInput.reset();
    }

    private static int vehicleId(Minecraft client) {
        if (client.player == null || !client.player.isAlive() || client.player.getVehicle() == null) return 0;
        return client.player.getVehicle().getId();
    }

    private static boolean mountedEligible(Minecraft client) {
        return enabled && client.getConnection() != null && confirmedVehicle
                && queriedVehicle != 0 && queriedVehicle == vehicleId(client)
                && client.gui.screen() == null;
    }

    public static boolean suppressAttack(Minecraft client) {
        return mountedEligible(client) && mounted.containsKey(ATTACK);
    }

    public static boolean suppressUse(Minecraft client) {
        return mountedEligible(client) && mounted.containsKey(USE);
    }

    private static void tickMounted(Minecraft client) {
        if (!enabled || client.getConnection() == null || mounted.isEmpty()) return;
        int vehicle = vehicleId(client);
        if (vehicle != queriedVehicle) {
            queriedVehicle = vehicle;
            confirmedVehicle = false;
            mountReplyReceived = false;
            queryTicks = 0;
            mountedInput.reset();
        }
        // A query can be rate-limited; retry until the server acknowledges this vehicle.
        if (vehicle != 0 && !mountReplyReceived && queryTicks++ % 10 == 0) {
            var query = ByteBuffer.allocate(6).put((byte) VERSION).put((byte) 3).putInt(vehicle);
            client.getConnection().send(new ServerboundCustomPayloadPacket(new Packet(query.array())));
        }
        if (!confirmedVehicle || vehicle == 0) return;
        boolean eligible = mountedEligible(client);
        var attack = mounted.get(ATTACK);
        var use = mounted.get(USE);
        if (attack != null) mouse(client, attack, client.options.keyAttack, 0, eligible);
        if (use != null) mouse(client, use, client.options.keyUse, 1, eligible);
        if (!eligible) {
            mountedInput.resetDirections();
            return;
        }
        var keys = new KeyMapping[] {client.options.keyUp, client.options.keyDown,
                client.options.keyLeft, client.options.keyRight};
        long now = System.nanoTime();
        for (int i = 0; i < keys.length; i++) {
            var action = mounted.get("moddedmc:double_tap/" + DIRECTIONS[i]);
            if (mountedInput.tap(i, keys[i].isDown(), now) && action != null) send(client, action, PRESSED);
        }
    }

    private static void mouse(Minecraft client, Definition action, KeyMapping key, int index, boolean eligible) {
        boolean clicked = eligible && key.consumeClick();
        if (eligible) while (key.consumeClick()) {}
        mountedInput.mouse(index, eligible && key.isDown(), clicked, phase -> send(client, action, phase));
    }

    private static final class Binding {
        final Definition definition;
        final KeyMapping mapping;
        final boolean ctrl;
        boolean down;
        int ticks;
        Binding(Definition definition, KeyMapping mapping, boolean ctrl) {
            this.definition = definition; this.mapping = mapping; this.ctrl = ctrl;
        }
    }

    private static void tick(Minecraft client) {
        if (!enabled || client.getConnection() == null || client.player == null) return;
        for (var binding : active.values()) {
            boolean clicked = binding.mapping.consumeClick();
            while (binding.mapping.consumeClick()) {}
            boolean eligible = client.gui.screen() == null && client.player.isAlive()
                    && (!binding.ctrl || client.hasControlDown());
            boolean down = eligible && binding.mapping.isDown();
            if (!binding.down && (down || (eligible && clicked))) {
                send(client, binding.definition, PRESSED);
                binding.ticks = 0;
                if (!down) send(client, binding.definition, RELEASED);
            }
            if (down && ++binding.ticks % 4 == 0) send(client, binding.definition, HELD);
            if (binding.down && !down) send(client, binding.definition, RELEASED);
            binding.down = down;
        }
        persist();
    }

    private static void send(Minecraft client, Definition action, int phase) {
        if ((action.phases & phase) == 0) return;
        byte[] id = action.id.getBytes(StandardCharsets.UTF_8);
        var data = ByteBuffer.allocate(4 + id.length);
        data.put((byte) VERSION).put((byte) 2).put((byte) phase).put((byte) id.length).put(id);
        client.getConnection().send(new ServerboundCustomPayloadPacket(new Packet(data.array())));
    }

    private static void persist() {
        boolean dirty = false;
        for (var entry : known.entrySet()) {
            String key = entry.getValue().saveString();
            if (!key.equals(saved.getProperty(entry.getKey()))) {
                saved.setProperty(entry.getKey(), key);
                dirty = true;
            }
        }
        if (dirty) {
            try {
                Files.createDirectories(SAVE.getParent());
                try (var output = Files.newOutputStream(SAVE)) { saved.store(output, "moddedMC player action keys"); }
            } catch (IOException ignored) {}
        }
    }

    private record Packet(byte[] bytes) implements CustomPacketPayload {
        private static final Type<Packet> TYPE = new Type<>(Identifier.fromNamespaceAndPath("moddedmc", "player_actions"));
        private static final StreamCodec<FriendlyByteBuf, Packet> CODEC = StreamCodec.of(
                (buf, packet) -> buf.writeBytes(packet.bytes()),
                buf -> {
                    if (buf.readableBytes() > MAX_PACKET) {
                        buf.skipBytes(buf.readableBytes());
                        return new Packet(new byte[0]);
                    }
                    byte[] bytes = new byte[buf.readableBytes()];
                    buf.readBytes(bytes);
                    return new Packet(bytes);
                });
        @Override public Type<Packet> type() { return TYPE; }
    }
}
