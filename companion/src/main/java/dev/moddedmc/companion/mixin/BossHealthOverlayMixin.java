package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.WaveOverlay;
import java.util.Collection;
import java.util.Map;
import java.util.UUID;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.BossHealthOverlay;
import net.minecraft.client.gui.components.LerpingBossEvent;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.game.ClientboundBossEventPacket;
import net.minecraft.util.Mth;
import org.spongepowered.asm.mixin.Final;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.Redirect;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(BossHealthOverlay.class)
public abstract class BossHealthOverlayMixin {
    @Shadow @Final private Minecraft minecraft;
    @Shadow @Final private Map<UUID, LerpingBossEvent> events;
    @Unique private WaveOverlay.Style moddedmc$style;
    @Unique private LerpingBossEvent moddedmc$event;
    @Unique private Component moddedmc$title;
    @Unique private float moddedmc$progress, moddedmc$previous, moddedmc$target;
    @Unique private long moddedmc$time;

    @Unique private void moddedmc$advance() {
        if (moddedmc$style == null || minecraft.level == null) return;
        long now = minecraft.level.getGameTime();
        long elapsed = Math.max(0, now - moddedmc$time);
        if (elapsed == 0) return;
        moddedmc$previous = WaveOverlay.advance(moddedmc$progress, moddedmc$target, moddedmc$style.smoothing(), elapsed - 1);
        moddedmc$progress = WaveOverlay.advance(moddedmc$previous, moddedmc$target, moddedmc$style.smoothing(), 1);
        moddedmc$time = now;
    }

    @Inject(method = "update", at = @At("RETURN"))
    private void moddedmc$update(ClientboundBossEventPacket packet, CallbackInfo ci) {
        moddedmc$advance();
        moddedmc$event = null;
        for (var event : events.values()) {
            var style = moddedmc$style != null && event.getName().getString().startsWith(moddedmc$style.prefix())
                    ? moddedmc$style : WaveOverlay.find(minecraft, event.getName());
            if (style == null) continue;
            if (moddedmc$style == null && minecraft.level != null) moddedmc$time = minecraft.level.getGameTime();
            moddedmc$style = style;
            moddedmc$event = event;
            moddedmc$title = event.getName();
            moddedmc$target = ((LerpingBossEventAccessor) event).moddedmc$targetProgress();
            return;
        }
        moddedmc$target = 0;
    }

    @Redirect(method = "extractRenderState", at = @At(value = "INVOKE", target = "Ljava/util/Map;values()Ljava/util/Collection;"))
    private Collection<LerpingBossEvent> moddedmc$nativeBars(Map<UUID, LerpingBossEvent> bars) {
        if (moddedmc$event == null) return bars.values();
        return bars.values().stream().filter(event -> event != moddedmc$event).toList();
    }

    @Inject(method = "extractRenderState", at = @At("RETURN"))
    private void moddedmc$draw(GuiGraphicsExtractor graphics, CallbackInfo ci) {
        if (moddedmc$style == null) return;
        moddedmc$advance();
        if (moddedmc$event == null && moddedmc$progress < .01f) {
            moddedmc$style = null;
            moddedmc$progress = moddedmc$previous = 0;
            return;
        }
        float delta = minecraft.getDeltaTracker().getGameTimeDeltaPartialTick(false);
        WaveOverlay.render(minecraft, graphics, moddedmc$style, moddedmc$title,
                Mth.lerp(Math.clamp(delta, 0, 1), moddedmc$previous, moddedmc$progress));
    }

    @Inject(method = "reset", at = @At("RETURN"))
    private void moddedmc$reset(CallbackInfo ci) {
        moddedmc$style = null;
        moddedmc$event = null;
        moddedmc$progress = moddedmc$previous = moddedmc$target = 0;
    }
}
