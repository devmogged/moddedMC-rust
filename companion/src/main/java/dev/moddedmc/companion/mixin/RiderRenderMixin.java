package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.rig.RiderSeats;
import net.minecraft.client.renderer.entity.EntityRenderer;
import net.minecraft.client.renderer.entity.state.EntityRenderState;
import net.minecraft.world.entity.Entity;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(EntityRenderer.class)
abstract class RiderRenderMixin {
    @Inject(method = "extractRenderState(Lnet/minecraft/world/entity/Entity;Lnet/minecraft/client/renderer/entity/state/EntityRenderState;F)V", at = @At("TAIL"))
    private void moddedmc$animatedSeat(Entity rider, EntityRenderState state, float delta, CallbackInfo ci) {
        var seat = RiderSeats.position(rider.getVehicle(), rider, delta);
        if (seat != null) {
            state.x = seat.x;
            state.y = seat.y;
            state.z = seat.z;
            state.passengerOffset = null;
        }
    }
}
