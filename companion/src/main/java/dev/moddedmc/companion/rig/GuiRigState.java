package dev.moddedmc.companion.rig;

import net.minecraft.client.renderer.entity.state.EntityRenderState;

/** An isolated GUI entity pass; does not create a network or world entity. */
public final class GuiRigState extends EntityRenderState {
    public final RigFrame frame;

    public GuiRigState(RigFrame frame) {
        this.frame = frame;
    }
}
