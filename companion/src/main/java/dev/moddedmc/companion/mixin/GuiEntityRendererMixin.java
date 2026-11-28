package dev.moddedmc.companion.mixin;

import com.mojang.blaze3d.vertex.PoseStack;
import dev.moddedmc.companion.rig.GuiRigState;
import net.minecraft.client.gui.render.pip.GuiEntityRenderer;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.entity.EntityRenderDispatcher;
import net.minecraft.client.renderer.entity.state.EntityRenderState;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Redirect;

@Mixin(GuiEntityRenderer.class)
abstract class GuiEntityRendererMixin {
    @Redirect(method = "renderToTexture(Lnet/minecraft/client/renderer/state/gui/pip/GuiEntityRenderState;Lcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/SubmitNodeCollector;)V",
            at = @At(value = "INVOKE", target = "Lnet/minecraft/client/renderer/entity/EntityRenderDispatcher;submit(Lnet/minecraft/client/renderer/entity/state/EntityRenderState;Lnet/minecraft/client/renderer/state/level/CameraRenderState;DDDLcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/SubmitNodeCollector;)V"))
    private void moddedmc$submitPreview(EntityRenderDispatcher dispatcher, EntityRenderState state,
            CameraRenderState camera, double x, double y, double z, PoseStack pose, SubmitNodeCollector collector) {
        if (state instanceof GuiRigState preview) {
            preview.frame.submit(pose, collector, camera);
        } else {
            dispatcher.submit(state, camera, x, y, z, pose, collector);
        }
    }
}
