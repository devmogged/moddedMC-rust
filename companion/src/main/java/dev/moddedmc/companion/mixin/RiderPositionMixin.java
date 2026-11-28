package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.rig.RiderSeats;
import net.minecraft.world.entity.Entity;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(Entity.class)
abstract class RiderPositionMixin {
    @Inject(method = "positionRider(Lnet/minecraft/world/entity/Entity;)V", at = @At("HEAD"), cancellable = true)
    private void moddedmc$animatedSeat(Entity rider, CallbackInfo ci) {
        Entity mount = (Entity) (Object) this;
        if (!mount.level().isClientSide()) return;
        var seat = RiderSeats.position(mount, rider, 1f);
        if (seat != null) {
            rider.setPos(seat);
            ci.cancel();
        }
    }
}
