package dev.moddedmc.companion;

import java.util.Set;
import net.minecraft.client.renderer.block.BlockAndTintGetter;
import net.minecraft.client.resources.model.geometry.BakedQuad;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.resources.Identifier;
import net.minecraft.world.level.block.state.BlockState;

public final class TrapdoorConnectedTextures {
    private static final int TILE_COUNT = 8;

    private TrapdoorConnectedTextures() {}

    public static BakedQuad apply(BakedQuad quad, BlockAndTintGetter level, BlockPos pos, BlockState state,
            Set<Identifier> textures, int[] faceTiles) {
        var source = quad.materialInfo().sprite();
        if (!textures.contains(source.contents().name())) return quad;
        var target = ConnectedTextures.target(source);
        if (target == null) return quad;

        Direction face = quad.direction();
        int index = face.get3DDataValue();
        if (faceTiles[index] < 0) faceTiles[index] = tile(level, pos, state, face);
        return ConnectedTextures.retile(quad, target, faceTiles[index], TILE_COUNT);
    }

    private static int tile(BlockAndTintGetter level, BlockPos pos, BlockState state, Direction face) {
        Direction right = face.getAxis() == Direction.Axis.X ? Direction.SOUTH : Direction.WEST;
        if (face.getAxisDirection() == Direction.AxisDirection.POSITIVE || face == Direction.DOWN) {
            right = right.getOpposite();
        }
        Direction up = face.getAxis().isHorizontal() ? Direction.UP
                : face == Direction.DOWN ? Direction.SOUTH : Direction.NORTH;
        Direction left = right.getOpposite();
        Direction down = up.getOpposite();
        var above = pos.relative(up);
        var below = pos.relative(down);
        boolean u = TrapdoorCatalog.connects(state, level.getBlockState(above), up);
        boolean d = TrapdoorCatalog.connects(state, level.getBlockState(below), down);
        boolean l = TrapdoorCatalog.connects(state, level.getBlockState(pos.relative(left)), left);
        boolean r = TrapdoorCatalog.connects(state, level.getBlockState(pos.relative(right)), right);
        // TrapdoorCTBehaviour uses the horizontal offset when testing corners.
        return ConnectedTextures.omni(u, d, l, r,
                u && l && TrapdoorCatalog.connects(state, level.getBlockState(above.relative(left)), left),
                u && r && TrapdoorCatalog.connects(state, level.getBlockState(above.relative(right)), right),
                d && l && TrapdoorCatalog.connects(state, level.getBlockState(below.relative(left)), left),
                d && r && TrapdoorCatalog.connects(state, level.getBlockState(below.relative(right)), right));
    }
}
