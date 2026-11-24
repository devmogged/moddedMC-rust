package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.rig.RigFrame;
import net.minecraft.client.renderer.entity.state.DisplayEntityRenderState;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;

@Mixin(DisplayEntityRenderState.class)
abstract class DisplayRenderStateMixin implements RigFrame.Holder {
    @Unique
    private RigFrame moddedmc$rigFrame;

    @Override
    public RigFrame moddedmc$rigFrame() {
        return moddedmc$rigFrame;
    }

    @Override
    public void moddedmc$rigFrame(RigFrame frame) {
        moddedmc$rigFrame = frame;
    }
}
