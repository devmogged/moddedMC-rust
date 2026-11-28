package dev.moddedmc.companion;

import com.google.gson.JsonParser;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;
import java.util.UUID;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.resources.sounds.AbstractTickableSoundInstance;
import net.minecraft.client.resources.sounds.SoundInstance;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.entity.Entity;

/** Timed positional sounds attached to tracked entities, independent of carrier audio. */
public final class EntityAudio {
    private record Key(UUID entity, Identifier sound) {}
    private static final Map<Key, Entry> ACTIVE = new HashMap<>();
    private static ClientLevel level;

    private static final class Entry {
        final UUID playback;
        final int id;
        final SoundSource source;
        final float volume, pitch;
        final boolean loop;
        final long end;
        boolean triggered;
        boolean duplicateTrigger;
        MovingSound sound;

        Entry(UUID playback, int id, SoundSource source, float volume, float pitch, boolean loop, long end) {
            this.playback = playback;
            this.id = id;
            this.source = source;
            this.volume = volume;
            this.pitch = pitch;
            this.loop = loop;
            this.end = end;
        }
    }

    static void register() {
        PayloadTypeRegistry.clientboundPlay().register(Packet.TYPE, Packet.CODEC);
        ClientPlayNetworking.registerGlobalReceiver(Packet.TYPE, (packet, context) -> {
            var client = context.client();
            updateLevel(client);
            if (!Companion.negotiated() || level == null) return;
            try {
                var data = JsonParser.parseString(new String(packet.bytes(), StandardCharsets.UTF_8)).getAsJsonObject();
                var uuid = UUID.fromString(data.get("uuid").getAsString());
                var sound = Identifier.tryParse(data.get("key").getAsString());
                if (sound == null) return;
                var key = new Key(uuid, sound);
                if (data.has("stop") && data.get("stop").getAsBoolean()) {
                    stop(client, ACTIVE.remove(key));
                    return;
                }
                var playback = UUID.fromString(data.get("playback").getAsString());
                int id = data.get("entity").getAsInt();
                int source = data.get("source").getAsInt();
                int duration = data.get("duration").getAsInt();
                float volume = data.get("volume").getAsFloat();
                float pitch = data.get("pitch").getAsFloat();
                boolean loop = data.get("loop").getAsBoolean();
                if (source < 0 || source >= SoundSource.values().length || duration <= 0
                        || !Float.isFinite(volume) || volume < 0 || !Float.isFinite(pitch) || pitch <= 0) return;
                var previous = ACTIVE.get(key);
                if (previous != null && previous.playback.equals(playback)) {
                    // A leave/re-enter transition can arrive before the client tick
                    // discards the old sound. Consume its paired vanilla trigger.
                    previous.duplicateTrigger = true;
                    return;
                }
                stop(client, previous);
                var entry = new Entry(playback, id, SoundSource.values()[source], volume, pitch, loop,
                        level.getGameTime() + duration);
                ACTIVE.put(key, entry);
            } catch (RuntimeException ignored) {}
        });
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            updateLevel(client);
            if (level == null || client.isPaused()) return;
            ACTIVE.entrySet().removeIf(pair -> {
                var entry = pair.getValue();
                if (level.getGameTime() >= entry.end || (entry.sound != null && entry.sound.isStopped())) {
                    stop(client, entry);
                    return true;
                }
                if (entry.triggered && entry.sound == null) tryStart(client, pair.getKey(), entry);
                return false;
            });
        });
        ClientPlayConnectionEvents.DISCONNECT.register((listener, client) -> clear(client));
    }

    private static void stop(Minecraft client, Entry entry) {
        if (entry != null && entry.sound != null) client.getSoundManager().stop(entry.sound);
    }

    private static void clear(Minecraft client) {
        for (var entry : ACTIVE.values()) stop(client, entry);
        ACTIVE.clear();
        level = null;
    }

    private static void updateLevel(Minecraft client) {
        if (level != client.level) {
            clear(client);
            level = client.level;
        }
    }

    private static Entity find(Key key, Entry entry) {
        var entity = level == null ? null : level.getEntity(entry.id);
        return entity != null && entity.getUUID().equals(key.entity) && entity.isAlive() && !entity.isRemoved()
                ? entity : null;
    }

    private static void tryStart(Minecraft client, Key key, Entry entry) {
        var entity = find(key, entry);
        if (entity == null) return;
        entry.sound = new MovingSound(key, entry, entity);
        client.getSoundManager().play(entry.sound);
    }

    /** Consume only the vanilla packet immediately following a prepared playback. */
    public static boolean start(int id, Identifier sound, SoundSource source) {
        var client = Minecraft.getInstance();
        updateLevel(client);
        for (var pair : ACTIVE.entrySet()) {
            var entry = pair.getValue();
            if (entry.duplicateTrigger && entry.id == id && entry.source == source
                    && pair.getKey().sound.equals(sound)) {
                entry.duplicateTrigger = false;
                return true;
            }
            if (!entry.triggered && entry.id == id && entry.source == source
                    && pair.getKey().sound.equals(sound) && level.getGameTime() < entry.end) {
                entry.triggered = true;
                tryStart(client, pair.getKey(), entry);
                return true;
            }
        }
        return false;
    }

    private static final class MovingSound extends AbstractTickableSoundInstance {
        private final Key key;
        private final Entry entry;

        MovingSound(Key key, Entry entry, Entity entity) {
            super(SoundEvent.createVariableRangeEvent(key.sound), entry.source, SoundInstance.createUnseededRandom());
            this.key = key;
            this.entry = entry;
            looping = entry.loop;
            relative = false;
            attenuation = Attenuation.LINEAR;
            delay = 0;
            volume = entry.volume;
            pitch = entry.pitch;
            move(entity);
        }

        private void move(Entity entity) {
            x = entity.getX();
            y = entity.getY();
            z = entity.getZ();
        }

        @Override public void tick() {
            var entity = find(key, entry);
            if (entity == null || level.getGameTime() >= entry.end) stop();
            else move(entity);
        }
    }

    private record Packet(byte[] bytes) implements CustomPacketPayload {
        private static final Type<Packet> TYPE = new Type<>(Identifier.fromNamespaceAndPath("moddedmc", "entity_audio"));
        private static final StreamCodec<FriendlyByteBuf, Packet> CODEC = StreamCodec.of(
                (buf, packet) -> buf.writeBytes(packet.bytes),
                buf -> {
                    if (buf.readableBytes() > 1024) {
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
