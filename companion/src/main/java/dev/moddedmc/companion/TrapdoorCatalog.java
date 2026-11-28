package dev.moddedmc.companion;

import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.HashSet;
import java.util.Set;
import java.util.zip.ZipFile;
import net.minecraft.core.Direction;
import net.minecraft.resources.Identifier;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.TrapDoorBlock;
import net.minecraft.world.level.block.state.BlockState;
import org.slf4j.LoggerFactory;

record TrapdoorCatalog(Set<BlockState> states, Set<Identifier> textures) {
    static final TrapdoorCatalog EMPTY = new TrapdoorCatalog(Set.of(), Set.of());
    private static final String ENTRY = "assets/moddedmc/moddedmc/trapdoors.json";
    private static final int MAX_BYTES = 1024;

    static TrapdoorCatalog load(Path verifiedPack) {
        try (var zip = new ZipFile(verifiedPack.toFile())) {
            var entry = zip.getEntry(ENTRY);
            if (entry == null) return EMPTY;
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
                return new TrapdoorCatalog(Set.copyOf(states), loadTextures(zip));
            }
        } catch (IOException | RuntimeException error) {
            LoggerFactory.getLogger("moddedmc-companion").warn("Could not read server trapdoors", error);
            return EMPTY;
        }
    }

    private static Set<Identifier> loadTextures(ZipFile zip) throws IOException {
        var entry = zip.getEntry("assets/moddedmc/moddedmc/trapdoor-textures.json");
        if (entry == null) return Set.of();
        try (var input = zip.getInputStream(entry)) {
            var bytes = input.readNBytes(MAX_BYTES + 1);
            if (bytes.length > MAX_BYTES) throw new IOException("Trapdoor texture list is too large");
            var ids = JsonParser.parseString(new String(bytes, StandardCharsets.UTF_8)).getAsJsonArray();
            var textures = new HashSet<Identifier>();
            for (var id : ids) textures.add(Identifier.parse(id.getAsString()));
            return Set.copyOf(textures);
        }
    }

    static boolean connects(BlockState state, BlockState neighbor, Direction direction) {
        if (state.getBlock() != neighbor.getBlock()
                || state.getValue(TrapDoorBlock.POWERED) != neighbor.getValue(TrapDoorBlock.POWERED)) return false;
        boolean open = state.getValue(TrapDoorBlock.OPEN);
        if (open != neighbor.getValue(TrapDoorBlock.OPEN)) return false;
        if (!open) {
            boolean sameHalf = state.getValue(TrapDoorBlock.HALF) == neighbor.getValue(TrapDoorBlock.HALF);
            return sameHalf != (direction.getAxis() == Direction.Axis.Y);
        }
        Direction facing = state.getValue(TrapDoorBlock.FACING);
        Direction other = neighbor.getValue(TrapDoorBlock.FACING);
        return facing == other ? direction.getAxis() != facing.getAxis()
                : facing.getOpposite() == other && direction.getAxis() == facing.getAxis();
    }
}
