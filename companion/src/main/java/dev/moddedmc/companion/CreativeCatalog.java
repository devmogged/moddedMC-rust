package dev.moddedmc.companion;

import io.netty.buffer.Unpooled;
import java.io.IOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.zip.ZipFile;
import net.fabricmc.fabric.api.creativetab.v1.FabricCreativeModeTab;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

final class CreativeCatalog {
    private static final Logger LOGGER = LoggerFactory.getLogger("moddedmc-companion");
    private static final String ENTRY = "assets/moddedmc/moddedmc/creative-v1.bin";
    private static final int MAX_BYTES = 512 * 1024;
    private static final int MAX_ENTRIES = 4096;

    static void register() {
        Registry.register(BuiltInRegistries.CREATIVE_MODE_TAB,
                Identifier.fromNamespaceAndPath("moddedmc", "server_mods"),
                FabricCreativeModeTab.builder()
                        .title(Component.literal("Server Mods"))
                        .icon(() -> new ItemStack(Items.COMMAND_BLOCK))
                        .displayItems((params, output) -> {
                            byte[] bytes = Companion.creativeCatalog();
                            if (bytes != null && params.holders() instanceof RegistryAccess registries) {
                                decode(bytes, registries).forEach(output::accept);
                            }
                        })
                        .build());
    }

    static byte[] load(Path verifiedPack) {
        try (var zip = new ZipFile(verifiedPack.toFile())) {
            var entry = zip.getEntry(ENTRY);
            if (entry == null || entry.getSize() > MAX_BYTES) return null;
            try (var input = zip.getInputStream(entry)) {
                byte[] bytes = input.readNBytes(MAX_BYTES + 1);
                return bytes.length <= MAX_BYTES ? bytes : null;
            }
        } catch (IOException error) {
            LOGGER.warn("Could not read the server's Creative catalog", error);
            return null;
        }
    }

    private static List<ItemStack> decode(byte[] bytes, RegistryAccess registries) {
        var buffer = new RegistryFriendlyByteBuf(Unpooled.wrappedBuffer(bytes), registries);
        try {
            int count = buffer.readVarInt();
            if (count < 1 || count > MAX_ENTRIES) return List.of();
            var stacks = new ArrayList<ItemStack>(count);
            for (int i = 0; i < count; i++) {
                int length = buffer.readVarInt();
                if (length < 1 || length > buffer.readableBytes()) return List.of();
                var entry = new RegistryFriendlyByteBuf(buffer.readSlice(length), registries);
                var stack = ItemStack.OPTIONAL_UNTRUSTED_STREAM_CODEC.decode(entry);
                if (stack.getCount() != 1 || entry.isReadable()) return List.of();
                stacks.add(stack);
            }
            return buffer.isReadable() ? List.of() : stacks;
        } catch (RuntimeException error) {
            LOGGER.warn("Could not decode the server's Creative catalog", error);
            return List.of();
        } finally {
            buffer.release();
        }
    }
}
