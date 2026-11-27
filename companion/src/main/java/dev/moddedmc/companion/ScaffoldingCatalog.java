package dev.moddedmc.companion;

import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;
import java.util.zip.ZipFile;
import net.minecraft.core.BlockPos;
import net.minecraft.core.component.DataComponents;
import net.minecraft.resources.Identifier;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.TrapDoorBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.shapes.CollisionContext;
import net.minecraft.world.phys.shapes.EntityCollisionContext;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;
import org.slf4j.LoggerFactory;

public final class ScaffoldingCatalog {
    private static final String ENTRY = "assets/moddedmc/moddedmc/scaffolding.json";
    private static final int MAX_BYTES = 4096;
    private static final VoxelShape HALF = Block.column(16, 8, 16);
    private static final VoxelShape LEGS = Shapes.or(
            Block.box(0, 0, 0, 2, 16, 2), Block.box(0, 0, 14, 2, 16, 16),
            Block.box(14, 0, 0, 16, 16, 2), Block.box(14, 0, 14, 16, 16, 16));
    private static final VoxelShape FULL = Shapes.or(HALF, LEGS);
    private static final VoxelShape STABLE = Shapes.or(Block.column(16, 14, 16), LEGS);

    private ScaffoldingCatalog() {}

    public record Entry(boolean bottom, Identifier itemModel, Map<Identifier, Integer> connected) {
        public VoxelShape collision(BlockPos pos, CollisionContext context) {
            if (bottom) return HALF;
            return !context.isPlacement() && !context.isDescending()
                    && context.isAbove(Shapes.block(), pos, true) ? STABLE : Shapes.empty();
        }

        public VoxelShape outline(CollisionContext context) {
            if (bottom) return HALF;
            if (context instanceof EntityCollisionContext hit && hit.getEntity() instanceof LivingEntity entity
                    && itemModel.equals(entity.getMainHandItem().get(DataComponents.ITEM_MODEL))) {
                return Shapes.block();
            }
            return FULL;
        }
    }

    static Map<BlockState, Entry> load(Path verifiedPack) {
        try (var zip = new ZipFile(verifiedPack.toFile())) {
            var entry = zip.getEntry(ENTRY);
            if (entry == null) return Map.of();
            try (var input = zip.getInputStream(entry)) {
                byte[] bytes = input.readNBytes(MAX_BYTES + 1);
                if (bytes.length > MAX_BYTES) throw new IOException("Scaffolding catalog is too large");
                var entries = JsonParser.parseString(new String(bytes, StandardCharsets.UTF_8)).getAsJsonObject();
                var states = new HashMap<BlockState, Entry>();
                for (var pair : entries.entrySet()) {
                    int id = Integer.parseInt(pair.getKey());
                    var carrier = Block.stateById(id);
                    var shape = pair.getValue().getAsJsonArray();
                    if (!(carrier.getBlock() instanceof TrapDoorBlock) || Block.getId(carrier) != id
                            || shape.size() < 2 || shape.size() > 3 || !shape.get(0).getAsJsonPrimitive().isBoolean()
                            || !shape.get(1).getAsJsonPrimitive().isString()) {
                        throw new IOException("Invalid scaffolding carrier " + id);
                    }
                    var item = Identifier.parse(shape.get(1).getAsString());
                    var shifts = new HashMap<Identifier, Integer>();
                    if (shape.size() == 3) {
                        for (var shift : shape.get(2).getAsJsonObject().entrySet()) {
                            int tiles = shift.getValue().getAsInt();
                            if (tiles != 2 && tiles != 8) throw new IOException("Invalid scaffolding tile count");
                            shifts.put(Identifier.parse(shift.getKey()), tiles);
                        }
                    }
                    states.put(carrier, new Entry(shape.get(0).getAsBoolean(), item, Map.copyOf(shifts)));
                }
                return Map.copyOf(states);
            }
        } catch (IOException | RuntimeException error) {
            LoggerFactory.getLogger("moddedmc-companion").warn("Could not read scaffolding shapes", error);
            return Map.of();
        }
    }
}
