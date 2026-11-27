package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.PlayerActions;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.ModifyVariable;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(Minecraft.class)
abstract class MountedInputMixin {
    @Inject(method = "startAttack", at = @At("HEAD"), cancellable = true)
    private void moddedmc$attack(CallbackInfoReturnable<Boolean> cir) {
        if (PlayerActions.suppressAttack((Minecraft) (Object) this)) cir.setReturnValue(false);
    }

    @ModifyVariable(method = "continueAttack", at = @At("HEAD"), argsOnly = true)
    private boolean moddedmc$continueAttack(boolean attacking) {
        return attacking && !PlayerActions.suppressAttack((Minecraft) (Object) this);
    }

    @Inject(method = "startUseItem", at = @At("HEAD"), cancellable = true)
    private void moddedmc$use(CallbackInfo ci) {
        if (PlayerActions.suppressUse((Minecraft) (Object) this)) ci.cancel();
    }
}
