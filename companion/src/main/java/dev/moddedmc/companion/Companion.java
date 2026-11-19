package dev.moddedmc.companion;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.networking.v1.ClientConfigurationNetworking;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public final class Companion implements ClientModInitializer {
    static final Logger LOGGER = LoggerFactory.getLogger("moddedmc-companion");

    @Override
    public void onInitializeClient() {
        PayloadTypeRegistry.clientboundConfiguration().register(Negotiate.TYPE, Negotiate.CODEC);
        PayloadTypeRegistry.serverboundConfiguration().register(Negotiate.TYPE, Negotiate.CODEC);
        ClientConfigurationNetworking.registerGlobalReceiver(Negotiate.TYPE, (packet, context) -> {
            if (packet.bytes().length == 0) {
                context.packetListener().send(new ServerboundCustomPayloadPacket(new Negotiate(Protocol.hello())));
                return;
            }
            var answer = Protocol.answer(packet.bytes());
            if (answer != null) {
                LOGGER.info("moddedMC companion protocol {}, active capabilities {}", Protocol.VERSION, answer.flags());
            }
        });
    }

    private record Negotiate(byte[] bytes) implements CustomPacketPayload {
        private static final Type<Negotiate> TYPE = new Type<>(Identifier.fromNamespaceAndPath("moddedmc", "negotiate"));
        private static final StreamCodec<FriendlyByteBuf, Negotiate> CODEC = StreamCodec.of(
                (buf, packet) -> buf.writeBytes(packet.bytes()),
                buf -> {
                    if (buf.readableBytes() > 64) {
                        buf.skipBytes(buf.readableBytes());
                        return new Negotiate(new byte[] {-1});
                    }
                    var bytes = new byte[buf.readableBytes()];
                    buf.readBytes(bytes);
                    return new Negotiate(bytes);
                });

        @Override
        public Type<Negotiate> type() {
            return TYPE;
        }
    }
}
