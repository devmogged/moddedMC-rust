package dev.moddedmc.companion.mixin;

import com.mojang.blaze3d.vertex.PoseStack;
import dev.moddedmc.companion.rig.DisplayRigs;
import dev.moddedmc.companion.rig.ArcFrames;
import dev.moddedmc.companion.rig.RigFrame;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.entity.DisplayRenderer;
import net.minecraft.client.renderer.entity.state.DisplayEntityRenderState;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.world.entity.Display;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.Redirect;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(DisplayRenderer.class)
abstract class DisplayRendererMixin {
    @Unique
    private static final boolean moddedmc$hasGeckoLib = FabricLoader.getInstance().isModLoaded("geckolib");

    @Shadow
    protected abstract void submitInner(
            DisplayEntityRenderState state, PoseStack pose, SubmitNodeCollector collector, int light, float progress);

    @Inject(method = "extractRenderState(Lnet/minecraft/world/entity/Display;Lnet/minecraft/client/renderer/entity/state/DisplayEntityRenderState;F)V",
            at = @At("TAIL"))
    private void moddedmc$extractRig(Display entity, DisplayEntityRenderState state, float delta, CallbackInfo ci) {
        if (entity instanceof Display.ItemDisplay item) {
            RigFrame frame = ArcFrames.extract(item, delta);
            if (frame == null && moddedmc$hasGeckoLib) {
                frame = DisplayRigs.extract(item, state.lightCoords, delta);
            }
            ((RigFrame.Holder) state).moddedmc$rigFrame(frame);
        }
    }

    @Redirect(method = "submit(Lnet/minecraft/client/renderer/entity/state/DisplayEntityRenderState;Lcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/SubmitNodeCollector;Lnet/minecraft/client/renderer/state/level/CameraRenderState;)V",
            at = @At(value = "INVOKE", target = "Lnet/minecraft/client/renderer/entity/DisplayRenderer;submitInner(Lnet/minecraft/client/renderer/entity/state/DisplayEntityRenderState;Lcom/mojang/blaze3d/vertex/PoseStack;Lnet/minecraft/client/renderer/SubmitNodeCollector;IF)V"))
    private void moddedmc$submitRig(
            DisplayRenderer<?, ?, ?> renderer, DisplayEntityRenderState state, PoseStack pose,
            SubmitNodeCollector collector, int light, float progress, DisplayEntityRenderState outerState,
            PoseStack outerPose, SubmitNodeCollector outerCollector, CameraRenderState camera) {
        RigFrame frame = ((RigFrame.Holder) state).moddedmc$rigFrame();
        if (frame == null) {
            submitInner(state, pose, collector, light, progress);
        } else {
            frame.submit(pose, collector, camera);
        }
    }
}
