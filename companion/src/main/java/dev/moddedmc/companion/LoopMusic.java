package dev.moddedmc.companion;

import com.google.gson.JsonParser;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.minecraft.client.Minecraft;
import net.minecraft.client.resources.sounds.AbstractTickableSoundInstance;
import net.minecraft.client.resources.sounds.SoundInstance;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;

/** Adds relative playback and gain ramps to a server's keyed RECORDS sounds. */
public final class LoopMusic {
    private static final Map<Identifier, Lease> LEASES = new HashMap<>();

    private static final class Lease {
        float volume, fadeIn, fadeOut;
        long until;
        boolean started;
        LoopSound sound;
    }

    static void register() {
        PayloadTypeRegistry.clientboundPlay().register(Packet.TYPE, Packet.CODEC);
        ClientPlayNetworking.registerGlobalReceiver(Packet.TYPE, (packet, context) -> {
            if (!Companion.negotiated() || context.client().level == null) return;
            try {
                var data = JsonParser.parseString(new String(packet.bytes(), StandardCharsets.UTF_8)).getAsJsonObject();
                var key = Identifier.tryParse(data.get("key").getAsString());
                float volume = data.get("volume").getAsFloat();
                float fadeIn = data.get("fadeIn").getAsFloat();
                float fadeOut = data.get("fadeOut").getAsFloat();
                long ttl = data.get("ttl").getAsLong();
                if (key == null || !unit(volume) || !unit(fadeIn) || !unit(fadeOut) || ttl <= 0 || ttl > 0xffffffffL) return;
                var lease = LEASES.computeIfAbsent(key, ignored -> new Lease());
                lease.volume = volume;
                lease.fadeIn = fadeIn;
                lease.fadeOut = fadeOut;
                lease.until = Math.max(lease.until, now(context.client()) + ttl);
            } catch (RuntimeException ignored) {}
        });
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            if (client.level == null || client.player == null) {
                clear(client);
                return;
            }
            if (client.isPaused()) return;
            long time = now(client);
            LEASES.entrySet().removeIf(entry -> {
                var lease = entry.getValue();
                if (time >= lease.until) return lease.sound == null || lease.sound.isStopped()
                        || !client.getSoundManager().isActive(lease.sound);
                if (lease.started && (lease.sound == null || lease.sound.isStopped()
                        || !client.getSoundManager().isActive(lease.sound))) {
                    play(client, entry.getKey(), lease);
                }
                return false;
            });
        });
        ClientPlayConnectionEvents.DISCONNECT.register((listener, client) -> clear(client));
    }

    private static boolean unit(float value) { return value >= 0 && value <= 1; }
    private static long now(Minecraft client) { return client.level == null ? 0 : client.level.getGameTime(); }

    private static void clear(Minecraft client) {
        for (var lease : LEASES.values()) {
            if (lease.sound != null) client.getSoundManager().stop(lease.sound);
        }
        LEASES.clear();
    }

    private static void play(Minecraft client, Identifier key, Lease lease) {
        lease.sound = new LoopSound(key, lease);
        client.getSoundManager().play(lease.sound);
    }

    public static boolean start(Identifier key, SoundSource source) {
        var lease = LEASES.get(key);
        if (source != SoundSource.RECORDS || lease == null || now(Minecraft.getInstance()) >= lease.until) return false;
        lease.started = true;
        if (lease.sound == null || lease.sound.isStopped()) play(Minecraft.getInstance(), key, lease);
        return true;
    }

    public static boolean stop(Identifier key, SoundSource source) {
        var lease = LEASES.get(key);
        if (source != SoundSource.RECORDS || lease == null) return false;
        lease.until = now(Minecraft.getInstance());
        return true;
    }

    private static final class LoopSound extends AbstractTickableSoundInstance {
        private final Lease lease;
        private float gain;

        LoopSound(Identifier key, Lease lease) {
            super(SoundEvent.createVariableRangeEvent(key), SoundSource.RECORDS, SoundInstance.createUnseededRandom());
            this.lease = lease;
            looping = true;
            relative = true;
            attenuation = Attenuation.NONE;
            delay = 0;
            volume = 0;
            pitch = 1;
        }

        @Override public boolean canStartSilent() { return true; }

        @Override public void tick() {
            long time = now(Minecraft.getInstance());
            if (time < lease.until) gain = Math.min(1, gain + (lease.fadeIn == 0 ? 1 : lease.fadeIn));
            else gain = Math.max(0, gain - (lease.fadeOut == 0 ? 1 : lease.fadeOut));
            volume = gain * lease.volume;
            if (gain <= 0 && time >= lease.until) stop();
        }
    }

    private record Packet(byte[] bytes) implements CustomPacketPayload {
        private static final Type<Packet> TYPE = new Type<>(Identifier.fromNamespaceAndPath("moddedmc", "loop_music"));
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
