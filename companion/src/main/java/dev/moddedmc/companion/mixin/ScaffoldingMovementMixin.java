package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.Companion;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.level.block.state.BlockState;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Redirect;

@Mixin(LivingEntity.class)
abstract class ScaffoldingMovementMixin {
    @Redirect(method = "handleOnClimbable", at = @At(value = "INVOKE",
            target = "Lnet/minecraft/world/level/block/state/BlockState;is(Ljava/lang/Object;)Z"))
    private boolean moddedmc$descend(BlockState state, Object bambooScaffolding) {
        return state.getBlock() == bambooScaffolding || Companion.scaffold(state) != null;
    }
}
