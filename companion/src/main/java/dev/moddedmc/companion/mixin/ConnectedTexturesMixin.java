package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.Companion;
import dev.moddedmc.companion.ScaffoldingCatalog;
import dev.moddedmc.companion.ScaffoldingConnectedTextures;
import dev.moddedmc.companion.TrapdoorConnectedTextures;
import java.util.Arrays;
import java.util.Set;
import net.minecraft.client.renderer.block.BlockAndTintGetter;
import net.minecraft.client.renderer.block.BlockQuadOutput;
import net.minecraft.client.renderer.block.ModelBlockRenderer;
import net.minecraft.client.renderer.block.dispatch.BlockStateModel;
import net.minecraft.client.resources.model.geometry.BakedQuad;
import net.minecraft.core.BlockPos;
import net.minecraft.resources.Identifier;
import net.minecraft.world.level.block.state.BlockState;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.ModifyVariable;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(ModelBlockRenderer.class)
abstract class ConnectedTexturesMixin {
    @Unique private ScaffoldingCatalog.Entry moddedmc$scaffold;
    @Unique private Set<Identifier> moddedmc$trapdoorTextures = Set.of();
    @Unique private final int[] moddedmc$tiles = new int[6];

    @Inject(method = "tesselateBlock", at = @At("HEAD"))
    private void moddedmc$beginBlock(BlockQuadOutput output, float x, float y, float z, BlockAndTintGetter level,
            BlockPos pos, BlockState state, BlockStateModel model, long seed, CallbackInfo callback) {
        moddedmc$scaffold = Companion.scaffold(state);
        moddedmc$trapdoorTextures = Companion.trapdoorTextures(state);
        if (moddedmc$scaffold != null || !moddedmc$trapdoorTextures.isEmpty()) Arrays.fill(moddedmc$tiles, -1);
    }

    @ModifyVariable(method = "putQuadWithTint", at = @At("HEAD"), argsOnly = true)
    private BakedQuad moddedmc$replaceSprite(BakedQuad quad, BlockQuadOutput output, float x, float y, float z,
            BlockAndTintGetter level, BlockState state, BlockPos pos) {
        if (moddedmc$scaffold != null) {
            return ScaffoldingConnectedTextures.apply(quad, level, pos, moddedmc$scaffold, moddedmc$tiles);
        }
        return moddedmc$trapdoorTextures.isEmpty() ? quad
                : TrapdoorConnectedTextures.apply(quad, level, pos, state, moddedmc$trapdoorTextures, moddedmc$tiles);
    }
}
