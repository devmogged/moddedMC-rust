package dev.moddedmc.companion;

import net.minecraft.client.Minecraft;
import net.minecraft.client.model.geom.builders.UVPair;
import net.minecraft.client.renderer.texture.TextureAtlasSprite;
import net.minecraft.client.resources.model.geometry.BakedQuad;
import net.minecraft.client.resources.model.sprite.SpriteId;

final class ConnectedTextures {
    private ConnectedTextures() {}

    static TextureAtlasSprite target(TextureAtlasSprite source) {
        var id = source.contents().name().withSuffix("_connected");
        var target = Minecraft.getInstance().getAtlasManager().get(new SpriteId(source.atlasLocation(), id));
        return target.contents().name().equals(id) ? target : null;
    }

    static BakedQuad retile(BakedQuad quad, TextureAtlasSprite target, int tile, int size) {
        var material = quad.materialInfo();
        var source = material.sprite();
        var shifted = new BakedQuad.MaterialInfo(target, material.layer(), material.itemRenderType(),
                material.itemGlintRenderType(), material.itemGlintSpecialRenderType(), material.tintIndex(),
                material.shadeDirectionOverride(), material.lightEmission());
        return new BakedQuad(quad.position0(), quad.position1(), quad.position2(), quad.position3(),
                shift(quad.packedUV0(), source, target, tile, size), shift(quad.packedUV1(), source, target, tile, size),
                shift(quad.packedUV2(), source, target, tile, size), shift(quad.packedUV3(), source, target, tile, size),
                quad.direction(), shifted);
    }

    private static long shift(long packed, TextureAtlasSprite source, TextureAtlasSprite target, int tile, int size) {
        float u = (UVPair.unpackU(packed) - source.getU0()) / (source.getU1() - source.getU0());
        float v = (UVPair.unpackV(packed) - source.getV0()) / (source.getV1() - source.getV0());
        return UVPair.pack(target.getU((u + tile % size) / size), target.getV((v + tile / size) / size));
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
