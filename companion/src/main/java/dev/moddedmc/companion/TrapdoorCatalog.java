package dev.moddedmc.companion;

import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.HashSet;
import java.util.Set;
import java.util.zip.ZipFile;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.TrapDoorBlock;
import net.minecraft.world.level.block.state.BlockState;
import org.slf4j.LoggerFactory;

final class TrapdoorCatalog {
    private static final String ENTRY = "assets/moddedmc/moddedmc/trapdoors.json";
    private static final int MAX_BYTES = 1024;

    private TrapdoorCatalog() {}

    static Set<BlockState> load(Path verifiedPack) {
        try (var zip = new ZipFile(verifiedPack.toFile())) {
            var entry = zip.getEntry(ENTRY);
            if (entry == null) return Set.of();
            try (var input = zip.getInputStream(entry)) {
                var bytes = input.readNBytes(MAX_BYTES + 1);
                if (bytes.length > MAX_BYTES) throw new IOException("Trapdoor catalog is too large");
                var ids = JsonParser.parseString(new String(bytes, StandardCharsets.UTF_8)).getAsJsonArray();
                var states = new HashSet<BlockState>();
                for (var value : ids) {
                    int id = value.getAsInt();
                    var state = Block.stateById(id);
                    if (Block.getId(state) != id || !(state.getBlock() instanceof TrapDoorBlock)) {
                        throw new IOException("Invalid trapdoor carrier " + id);
                    }
                    states.add(state);
                }
                return Set.copyOf(states);
            }
        } catch (IOException | RuntimeException error) {
            LoggerFactory.getLogger("moddedmc-companion").warn("Could not read server trapdoors", error);
            return Set.of();
        }
    }
}
