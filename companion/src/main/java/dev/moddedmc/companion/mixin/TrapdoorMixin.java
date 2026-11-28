package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.Companion;
import net.minecraft.core.Direction;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.BlockHitResult;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(BlockBehaviour.BlockStateBase.class)
abstract class TrapdoorMixin {
    @Inject(method = "useWithoutItem", at = @At("HEAD"), cancellable = true)
    private void moddedmc$use(Level level, Player player, BlockHitResult hit,
            CallbackInfoReturnable<InteractionResult> result) {
        if (level.isClientSide() && Companion.isServerTrapdoor((BlockState)(Object)this)) {
            result.setReturnValue(InteractionResult.SUCCESS_SERVER);
        }
    }

    @Inject(method = "skipRendering", at = @At("HEAD"), cancellable = true)
    private void moddedmc$hideFace(BlockState neighbor, Direction direction, CallbackInfoReturnable<Boolean> result) {
        var state = (BlockState)(Object)this;
        if (Companion.isServerTrapdoor(state)) {
            result.setReturnValue(Companion.connectsTrapdoor(state, neighbor, direction));
        }
    }
}
