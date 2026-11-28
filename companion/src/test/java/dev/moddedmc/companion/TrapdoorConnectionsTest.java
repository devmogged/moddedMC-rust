package dev.moddedmc.companion;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import net.minecraft.SharedConstants;
import net.minecraft.core.Direction;
import net.minecraft.server.Bootstrap;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.world.level.block.TrapDoorBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Half;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class TrapdoorConnectionsTest {
    private static BlockState carrier;

    @BeforeAll
    static void loadBlockStates() {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        carrier = BuiltInRegistries.BLOCK.getValue(Identifier.withDefaultNamespace("waxed_oxidized_copper_trapdoor"))
                .defaultBlockState();
    }

    @Test
    void closedTrapdoorsJoinTheSameHalfSidewaysAndOppositeHalvesVertically() {
        var bottom = carrier;
        var top = bottom.setValue(TrapDoorBlock.HALF, Half.TOP).setValue(TrapDoorBlock.WATERLOGGED, true);
        assertTrue(TrapdoorCatalog.connects(bottom, bottom, Direction.EAST));
        assertFalse(TrapdoorCatalog.connects(bottom, bottom, Direction.UP));
        assertFalse(TrapdoorCatalog.connects(bottom, top, Direction.EAST));
        assertTrue(TrapdoorCatalog.connects(bottom, top, Direction.DOWN));
    }

    @Test
    void openTrapdoorsJoinOppositeHingesAlongTheHingeAxis() {
        var north = carrier
                .setValue(TrapDoorBlock.OPEN, true).setValue(TrapDoorBlock.FACING, Direction.NORTH);
        var south = north.setValue(TrapDoorBlock.FACING, Direction.SOUTH);
        assertTrue(TrapdoorCatalog.connects(north, south, Direction.NORTH));
        assertFalse(TrapdoorCatalog.connects(north, south, Direction.EAST));
        assertTrue(TrapdoorCatalog.connects(north, north.setValue(TrapDoorBlock.HALF, Half.TOP), Direction.UP));
        assertFalse(TrapdoorCatalog.connects(north, north.setValue(TrapDoorBlock.POWERED, true), Direction.UP));
    }
}
