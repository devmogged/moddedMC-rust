package dev.moddedmc.companion;

import net.minecraft.client.renderer.block.BlockAndTintGetter;
import net.minecraft.client.resources.model.geometry.BakedQuad;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.resources.Identifier;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.phys.shapes.CollisionContext;

public final class ScaffoldingConnectedTextures {
    private ScaffoldingConnectedTextures() {}

    public static BakedQuad apply(BakedQuad quad, BlockAndTintGetter level, BlockPos pos,
            ScaffoldingCatalog.Entry scaffold, int[] faceTiles) {
        var source = quad.materialInfo().sprite();
        var texture = source.contents().name();
        Integer size = scaffold.connected().get(texture);
        var face = quad.direction();
        if (size == null || (size == 2) != face.getAxis().isHorizontal()) return quad;

        var target = ConnectedTextures.target(source);
        if (target == null) return quad;

        int faceIndex = face.get3DDataValue();
        if (faceTiles[faceIndex] < 0) {
            faceTiles[faceIndex] = scaffold.bottom() ? tile(level, pos, face, scaffold.itemModel(), size) : 0;
        }
        return ConnectedTextures.retile(quad, target, faceTiles[faceIndex], size);
    }

    private static int tile(BlockAndTintGetter level, BlockPos pos, Direction face, Identifier item, int size) {
        Direction right = face.getAxis() == Direction.Axis.X ? Direction.SOUTH : Direction.WEST;
        if (face.getAxisDirection() == Direction.AxisDirection.POSITIVE || face == Direction.DOWN) {
            right = right.getOpposite();
        }
        Direction up = face == Direction.DOWN ? Direction.SOUTH : Direction.NORTH;
        boolean left = connects(level, pos.relative(right.getOpposite()), face, item);
        boolean rightConnected = connects(level, pos.relative(right), face, item);
        if (size == 2) return (rightConnected ? 1 : 0) + (left ? 2 : 0);

        var above = pos.relative(up);
        var below = pos.relative(up.getOpposite());
        boolean upConnected = connects(level, above, face, item);
        boolean down = connects(level, below, face, item);
        return ConnectedTextures.omni(upConnected, down, left, rightConnected,
                upConnected && left && connects(level, above.relative(right.getOpposite()), face, item),
                upConnected && rightConnected && connects(level, above.relative(right), face, item),
                down && left && connects(level, below.relative(right.getOpposite()), face, item),
                down && rightConnected && connects(level, below.relative(right), face, item));
    }

    private static boolean connects(BlockAndTintGetter level, BlockPos target, Direction face, Identifier item) {
        var other = Companion.scaffold(level.getBlockState(target));
        if (!matchingBottom(other, item)) return false;
        if (face.getAxis().isHorizontal()) return true;
        var coveredBy = Companion.scaffold(level.getBlockState(target.relative(face)));
        return !matchingBottom(coveredBy, item)
                || !Block.isFaceFull(coveredBy.outline(CollisionContext.empty()), face.getOpposite());
    }

    private static boolean matchingBottom(ScaffoldingCatalog.Entry entry, Identifier item) {
        return entry != null && entry.bottom() && item.equals(entry.itemModel());
    }

}
