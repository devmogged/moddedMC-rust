package dev.moddedmc.companion;

import net.minecraft.client.Minecraft;
import net.minecraft.client.model.geom.builders.UVPair;
import net.minecraft.client.renderer.block.BlockAndTintGetter;
import net.minecraft.client.renderer.texture.TextureAtlasSprite;
import net.minecraft.client.resources.model.geometry.BakedQuad;
import net.minecraft.client.resources.model.sprite.SpriteId;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.resources.Identifier;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.phys.shapes.CollisionContext;

public final class ScaffoldingConnectedTextures {
    private ScaffoldingConnectedTextures() {}

    public static BakedQuad apply(BakedQuad quad, BlockAndTintGetter level, BlockPos pos,
            ScaffoldingCatalog.Entry scaffold, int[] faceTiles) {
        var material = quad.materialInfo();
        var source = material.sprite();
        var texture = source.contents().name();
        Integer size = scaffold.connected().get(texture);
        var face = quad.direction();
        if (size == null || (size == 2) != face.getAxis().isHorizontal()) return quad;

        var targetId = texture.withSuffix("_connected");
        var target = Minecraft.getInstance().getAtlasManager().get(new SpriteId(source.atlasLocation(), targetId));
        if (!target.contents().name().equals(targetId)) return quad;

        int faceIndex = face.get3DDataValue();
        if (faceTiles[faceIndex] < 0) {
            faceTiles[faceIndex] = scaffold.bottom() ? tile(level, pos, face, scaffold.itemModel(), size) : 0;
        }
        int tile = faceTiles[faceIndex];
        var shifted = new BakedQuad.MaterialInfo(target, material.layer(), material.itemRenderType(),
                material.itemGlintRenderType(), material.itemGlintSpecialRenderType(), material.tintIndex(),
                material.shadeDirectionOverride(), material.lightEmission());
        return new BakedQuad(quad.position0(), quad.position1(), quad.position2(), quad.position3(),
                shift(quad.packedUV0(), source, target, tile, size), shift(quad.packedUV1(), source, target, tile, size),
                shift(quad.packedUV2(), source, target, tile, size), shift(quad.packedUV3(), source, target, tile, size),
                face, shifted);
    }

    private static long shift(long packed, TextureAtlasSprite source, TextureAtlasSprite target, int tile, int size) {
        float u = (UVPair.unpackU(packed) - source.getU0()) / (source.getU1() - source.getU0());
        float v = (UVPair.unpackV(packed) - source.getV0()) / (source.getV1() - source.getV0());
        return UVPair.pack(target.getU((u + tile % size) / size), target.getV((v + tile / size) / size));
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
        return omni(upConnected, down, left, rightConnected,
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

    static int omni(boolean up, boolean down, boolean left, boolean right,
            boolean topLeft, boolean topRight, boolean bottomLeft, boolean bottomRight) {
        int borders = (up ? 0 : 1) + (down ? 0 : 1) + (left ? 0 : 1) + (right ? 0 : 1);
        int x = (up ? 1 : 0) + (down ? 2 : 0);
        int y = (left ? 1 : 0) + (right ? 2 : 0);
        if (borders == 0) {
            x += (topRight ? 1 : 0) + (topLeft ? 2 : 0);
            y += (bottomLeft ? 1 : 0) + (bottomRight ? 2 : 0);
        } else if (borders == 1) {
            if (!right && (topLeft || bottomLeft)) {
                y = 4;
                x = (bottomLeft ? 1 : 0) + (topLeft ? 2 : 0) - 1;
            } else if (!left && (topRight || bottomRight)) {
                y = 5;
                x = (bottomRight ? 1 : 0) + (topRight ? 2 : 0) - 1;
            } else if (!down && (topLeft || topRight)) {
                y = 6;
                x = (topLeft ? 1 : 0) + (topRight ? 2 : 0) - 1;
            } else if (!up && (bottomLeft || bottomRight)) {
                y = 7;
                x = (bottomLeft ? 1 : 0) + (bottomRight ? 2 : 0) - 1;
            }
        } else if (borders == 2 && (topLeft || topRight || bottomLeft || bottomRight)) {
            x += 3;
        }
        return x + 8 * y;
    }
}
