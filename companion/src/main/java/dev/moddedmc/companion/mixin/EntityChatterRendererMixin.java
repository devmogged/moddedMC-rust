package dev.moddedmc.companion.mixin;

import com.mojang.blaze3d.vertex.PoseStack;
import dev.moddedmc.companion.EntityChatter;
import java.util.WeakHashMap;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.entity.EntityRenderer;
import net.minecraft.client.renderer.entity.state.EntityRenderState;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.network.chat.Component;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(EntityRenderer.class)
abstract class EntityChatterRendererMixin {
    @Unique private final WeakHashMap<EntityRenderState, Component> moddedmc$chatter = new WeakHashMap<>();

    @Inject(method = "extractRenderState", at = @At("TAIL"))
    private void moddedmc$extractChatter(Entity entity, EntityRenderState state, float delta, CallbackInfo ci) {
        var text = EntityChatter.visible(entity, state.distanceToCameraSq);
        if (text == null) moddedmc$chatter.remove(state);
        else moddedmc$chatter.put(state, text);
    }

    @Inject(method = "submit", at = @At("TAIL"))
    private void moddedmc$submitChatter(EntityRenderState state, PoseStack pose, SubmitNodeCollector collector,
                                         CameraRenderState camera, CallbackInfo ci) {
        var text = moddedmc$chatter.get(state);
        if (text != null) {
            Vec3 anchor = state.nameTagAttachment == null ? new Vec3(0, state.boundingBoxHeight + 0.15, 0)
                    : state.nameTagAttachment.add(0, 0.15, 0);
            collector.submitNameTag(pose, anchor, 0, text, false, state.lightCoords, camera);
        }
    }
}
