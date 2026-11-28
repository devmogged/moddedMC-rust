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
import net.minecraft.client.resources.sounds.SimpleSoundInstance;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.world.entity.Entity;

/** Entity-scoped translated overhead text with a typewriter reveal and optional voice blip. */
public final class EntityChatter {
    private static final Map<Integer, Line> LINES = new HashMap<>();
    private static Object level;
    private static final class Line {
        final String key, name, sound;
        final long started = System.nanoTime();
        long until, lastBlip;
        int voiced;
        Line(String key, String name, String sound) {
            this.key = key; this.name = name; this.sound = sound;
        }
    }
    static void register() {
        PayloadTypeRegistry.clientboundPlay().register(Packet.TYPE, Packet.CODEC);
        ClientPlayNetworking.registerGlobalReceiver(Packet.TYPE, (packet, context) -> {
            if (!Companion.negotiated() || context.client().level == null) return;
            resetWorld(context.client());
            try {
                var data = JsonParser.parseString(new String(packet.bytes(), StandardCharsets.UTF_8)).getAsJsonObject();
                int entity = data.get("entity").getAsInt();
                String key = data.get("key").getAsString(), name = data.get("name").getAsString(), sound = data.get("sound").getAsString();
                int ttl = data.get("ttl").getAsInt();
                if (ttl <= 0 || key.isEmpty()) { LINES.remove(entity); return; }
                if (ttl > 1800 || key.length() > 256 || name.length() > 256 || sound.length() > 256) return;
                var old = LINES.get(entity);
                var line = old != null && old.key.equals(key) && old.name.equals(name) && old.sound.equals(sound)
                        ? old : new Line(key, name, sound);
                line.until = context.client().level.getGameTime() + ttl;
                LINES.put(entity, line);
            } catch (RuntimeException ignored) {}
        });
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            resetWorld(client);
            if (client.level != null) LINES.values().removeIf(line -> line.until <= client.level.getGameTime());
        });
        ClientPlayConnectionEvents.DISCONNECT.register((handler, client) -> LINES.clear());
    }
    private static void resetWorld(Minecraft client) {
        if (level != client.level) { level = client.level; LINES.clear(); }
    }
    public static Component visible(Entity entity, double distanceSq) {
        if (distanceSq > 256 || !Companion.negotiated()) return null;
        var line = LINES.get(entity.getId());
        if (line == null) return null;
        String text = Component.translatable(line.key).getString().replace("{name}", line.name);
        long now = System.nanoTime();
        int count = Math.min(text.codePointCount(0, text.length()), (int)((now - line.started) / 42_000_000L));
        if (count == 0) return null;
        if (now - line.lastBlip >= 34_000_000L && count > line.voiced) {
            while (line.voiced < count) {
                int i = line.voiced++;
                int cp = text.codePointAt(text.offsetByCodePoints(0, i));
                if ((i & 1) == 0 && !Character.isWhitespace(cp) && ".,!?;:()[]{}\"'".indexOf(cp) < 0) {
                    var sound = Identifier.tryParse(line.sound);
                    if (sound != null) Minecraft.getInstance().getSoundManager().play(
                            SimpleSoundInstance.forUI(SoundEvent.createVariableRangeEvent(sound),
                                    0.94f + (float)Math.random() * 0.13f, 0.42f));
                    line.lastBlip = now;
                    break;
                }
            }
        }
        return Component.literal(text.substring(0, text.offsetByCodePoints(0, count)));
    }
    private record Packet(byte[] bytes) implements CustomPacketPayload {
        static final Type<Packet> TYPE = new Type<>(Identifier.fromNamespaceAndPath("moddedmc", "entity_chatter"));
        static final StreamCodec<FriendlyByteBuf, Packet> CODEC = StreamCodec.of(
                (buf, packet) -> buf.writeBytes(packet.bytes),
                buf -> {
                    if (buf.readableBytes() > 4096) { buf.skipBytes(buf.readableBytes()); return new Packet(new byte[0]); }
                    byte[] bytes = new byte[buf.readableBytes()]; buf.readBytes(bytes); return new Packet(bytes);
                });
        @Override public Type<Packet> type() { return TYPE; }
    }
}
