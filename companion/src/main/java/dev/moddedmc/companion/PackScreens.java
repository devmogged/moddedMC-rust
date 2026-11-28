package dev.moddedmc.companion;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayNetworking;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

/** Connection-local standalone screen transport. */
final class PackScreens {
    private static final int MAX = 256 * 1024;
    private static final Gson GSON = new Gson();

    static void register() {
        PayloadTypeRegistry.clientboundPlay().register(Packet.TYPE, Packet.CODEC);
        PayloadTypeRegistry.serverboundPlay().register(Packet.TYPE, Packet.CODEC);
        ClientPlayConnectionEvents.JOIN.register((listener, sender, client) -> {
            if (Companion.negotiated()) listener.send(new ServerboundCustomPayloadPacket(new Packet(new byte[]{1, 0})));
        });
        ClientPlayNetworking.registerGlobalReceiver(Packet.TYPE, (packet, context) -> {
            var bytes = packet.bytes();
            if (bytes.length < 3 || bytes[0] != 1 || bytes[1] != 1) return;
            try {
                var open = JsonParser.parseString(new String(bytes, 2, bytes.length - 2, StandardCharsets.UTF_8)).getAsJsonObject();
                var id = Identifier.tryParse(open.get("id").getAsString());
                var token = open.get("token").getAsString();
                var state = open.getAsJsonObject("state");
                var reply = open.get("replyTo");
                String replyTo = reply == null || reply.isJsonNull() ? null : reply.getAsString();
                if (id == null || state == null || !token.matches("[0-9]{1,20}")) return;
                context.client().execute(() -> {
                    if (!Companion.negotiated() || context.client().getConnection() == null) return;
                    if (replyTo != null) {
                        if (context.client().gui.screen() instanceof StandalonePackScreen current
                                && current.matches(id) && current.hasToken(replyTo)) current.update(token, state);
                    } else {
                        var layout = StandaloneLayout.load(context.client(), id);
                        if (layout == null) return;
                        context.client().gui.setScreen(new StandalonePackScreen(id, layout, token, state));
                    }
                });
            } catch (RuntimeException ignored) {}
        });
    }

    static void click(String token, String action, String text) {
        var connection = net.minecraft.client.Minecraft.getInstance().getConnection();
        if (!Companion.negotiated() || connection == null) return;
        var body = new JsonObject();
        body.addProperty("token", token);
        body.addProperty("action", action);
        body.addProperty("text", text);
        byte[] json = GSON.toJson(body).getBytes(StandardCharsets.UTF_8);
        if (json.length > 1022) return;
        byte[] bytes = Arrays.copyOf(json, json.length + 2);
        System.arraycopy(bytes, 0, bytes, 2, json.length);
        bytes[0] = 1;
        bytes[1] = 2;
        connection.send(new ServerboundCustomPayloadPacket(new Packet(bytes)));
    }

    private record Packet(byte[] bytes) implements CustomPacketPayload {
        private static final Type<Packet> TYPE = new Type<>(Identifier.fromNamespaceAndPath("moddedmc", "pack_screen"));
        private static final StreamCodec<FriendlyByteBuf, Packet> CODEC = StreamCodec.of(
            (buf, packet) -> buf.writeBytes(packet.bytes),
            buf -> {
                if (buf.readableBytes() > MAX + 2) {
                    buf.skipBytes(buf.readableBytes());
                    return new Packet(new byte[0]);
                }
                var bytes = new byte[buf.readableBytes()];
                buf.readBytes(bytes);
                return new Packet(bytes);
            });
        @Override public Type<Packet> type() { return TYPE; }
    }
}
