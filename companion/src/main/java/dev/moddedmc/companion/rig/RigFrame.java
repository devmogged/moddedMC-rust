package dev.moddedmc.companion.rig;

import com.mojang.blaze3d.vertex.PoseStack;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.state.level.CameraRenderState;

@FunctionalInterface
public interface RigFrame {
    void submit(PoseStack pose, SubmitNodeCollector collector, CameraRenderState camera);

    interface Holder {
        RigFrame moddedmc$rigFrame();

        void moddedmc$rigFrame(RigFrame frame);
    }
}
