package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.LoopMusic;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientPacketListener;
import net.minecraft.network.protocol.game.ClientboundSoundPacket;
import net.minecraft.network.protocol.game.ClientboundStopSoundPacket;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(ClientPacketListener.class)
abstract class LoopMusicPacketMixin {
    @Inject(method = "handleSoundEvent", at = @At("HEAD"), cancellable = true)
    private void loopMusicStart(ClientboundSoundPacket packet, CallbackInfo ci) {
        if (Minecraft.getInstance().isSameThread()
                && LoopMusic.start(packet.getSound().value().location(), packet.getSource())) ci.cancel();
    }

    @Inject(method = "handleStopSoundEvent", at = @At("HEAD"), cancellable = true)
    private void loopMusicStop(ClientboundStopSoundPacket packet, CallbackInfo ci) {
        if (Minecraft.getInstance().isSameThread()
                && LoopMusic.stop(packet.getName(), packet.getSource())) ci.cancel();
    }
}
