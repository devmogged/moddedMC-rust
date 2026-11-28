package dev.moddedmc.companion;

import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.Proxy;
import java.net.URI;
import java.net.URISyntaxException;
import java.net.URL;
import java.nio.file.Path;
import java.util.Map;
import java.util.Set;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.networking.v1.ClientConfigurationConnectionEvents;
import net.fabricmc.fabric.api.client.networking.v1.ClientConfigurationNetworking;
import net.fabricmc.fabric.api.client.networking.v1.ClientPlayConnectionEvents;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.core.Direction;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;
import net.minecraft.world.level.block.TrapDoorBlock;
import net.minecraft.world.level.block.state.BlockState;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public final class Companion implements ClientModInitializer {
    private static final Logger LOGGER = LoggerFactory.getLogger("moddedmc-companion");
    private static volatile Session session;
    private static volatile boolean negotiated;
    private static volatile int capabilities;

    private static final class Session {
        private final Protocol.Offer offer;
        private volatile byte[] creativeCatalog;
        private volatile Map<BlockState, ScaffoldingCatalog.Entry> scaffolding = Map.of();
        private volatile TrapdoorCatalog trapdoors = TrapdoorCatalog.EMPTY;

        private Session(Protocol.Offer offer) {
            this.offer = offer;
        }
    }

    @Override
    public void onInitializeClient() {
        CreativeCatalog.register();
        PlayerActions.register();
        BlockDisplayUse.register();
        PackScreens.register();
        LoopMusic.register();
        EntityChatter.register();
        ClientPlayConnectionEvents.DISCONNECT.register((listener, client) -> clearSession());
        ClientConfigurationConnectionEvents.INIT.register((listener, client) -> clearSession());
        ClientConfigurationConnectionEvents.DISCONNECT.register((listener, client) -> clearSession());
        PayloadTypeRegistry.clientboundConfiguration().register(Negotiate.TYPE, Negotiate.CODEC);
        PayloadTypeRegistry.serverboundConfiguration().register(Negotiate.TYPE, Negotiate.CODEC);
        ClientConfigurationNetworking.registerGlobalReceiver(Negotiate.TYPE, (packet, context) -> {
            if (packet.bytes().length == 0) {
                context.packetListener().send(new ServerboundCustomPayloadPacket(new Negotiate(Protocol.hello())));
                return;
            }
            var answer = Protocol.answer(packet.bytes());
            if (answer != null) {
                capabilities = answer.flags();
                negotiated = true;
                session = answer.offer() == null ? null : new Session(answer.offer());
                PlayerActions.negotiated((answer.flags() & Protocol.PLAYER_ACTIONS) != 0);
                LOGGER.info("moddedMC companion protocol {}, active capabilities {}", Protocol.VERSION, answer.flags());
            }
        });
    }

    private static void clearSession() {
        session = null;
        negotiated = false;
        capabilities = 0;
    }

    public static boolean negotiated() {
        return negotiated;
    }

    static boolean canUseBlockDisplays() {
        var current = session;
        return current != null && !current.trapdoors.states().isEmpty()
                && (capabilities & Protocol.BLOCK_DISPLAY_USE) != 0;
    }

    static byte[] creativeCatalog() {
        var current = session;
        return current == null ? null : current.creativeCatalog;
    }

    public static ScaffoldingCatalog.Entry scaffold(BlockState state) {
        var current = session;
        return current == null ? null : current.scaffolding.get(state);
    }

    public static boolean isServerTrapdoor(BlockState state) {
        var current = session;
        return current != null && current.trapdoors.states().contains(state);
    }

    public static Set<Identifier> trapdoorTextures(BlockState state) {
        var current = session;
        if (current == null || !(state.getBlock() instanceof TrapDoorBlock)) return Set.of();
        var trapdoors = current.trapdoors;
        return trapdoors.textures().isEmpty() || !trapdoors.states().contains(state)
                ? Set.of() : trapdoors.textures();
    }

    public static boolean connectsTrapdoor(BlockState state, BlockState neighbor, Direction direction) {
        return TrapdoorCatalog.connects(state, neighbor, direction);
    }

    public static Path materialize(URL url, String zipSha1, Proxy proxy) throws IOException {
        var current = session;
        if (current == null || !current.offer.zipSha1().equals(zipSha1)) return null;
        URI base;
        try {
            base = url.toURI();
        } catch (URISyntaxException error) {
            throw new IOException("Invalid pack URL", error);
        }
        Path cache = FabricLoader.getInstance().getGameDir().resolve("moddedmc");
        Path local = PackCache.materialize(cache, current.offer.manifest(), key -> open(base.resolve(key), proxy));
        current.creativeCatalog = CreativeCatalog.load(local);
        current.scaffolding = ScaffoldingCatalog.load(local);
        current.trapdoors = TrapdoorCatalog.load(local);
        LOGGER.info("Loaded verified moddedMC asset pack {}", current.offer.manifest());
        return local;
    }

    private static InputStream open(URI uri, Proxy proxy) throws IOException {
        if (!"http".equalsIgnoreCase(uri.getScheme()) && !"https".equalsIgnoreCase(uri.getScheme())) {
            throw new IOException("Invalid pack URL");
        }
        var connection = (HttpURLConnection) uri.toURL().openConnection(proxy);
        connection.setConnectTimeout(5000);
        connection.setReadTimeout(5000);
        connection.setInstanceFollowRedirects(false);
        if (connection.getResponseCode() != 200) {
            connection.disconnect();
            throw new IOException("Pack HTTP request failed");
        }
        return connection.getInputStream();
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
