package dev.moddedmc.companion;

import com.google.gson.JsonParser;
import java.nio.charset.StandardCharsets;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.renderer.RenderPipelines;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;

/** A pack can replace a leased vanilla boss bar with its own HUD atlas. */
public final class WaveOverlay {
    public record Style(String prefix, String titleKey, Identifier texture, int width, int height, int textureHeight,
                        int fillV, float smoothing) {}

    public static Style find(Minecraft client, Component title) {
        if (!Companion.negotiated()) return null;
        var manager = client.getResourceManager();
        for (var entry : manager.listResources("moddedmc", id -> id.getPath().equals("moddedmc/screens.json")).entrySet()) {
            try (var stream = entry.getValue().open()) {
                byte[] bytes = stream.readNBytes(256 * 1024 + 1);
                if (bytes.length > 256 * 1024) continue;
                var catalog = JsonParser.parseString(new String(bytes, StandardCharsets.UTF_8)).getAsJsonObject();
                for (var value : catalog.entrySet()) {
                    var data = value.getValue().getAsJsonObject();
                    if (data.get("version").getAsInt() != 3) continue;
                    String prefix = data.get("titlePrefix").getAsString();
                    if (prefix.isEmpty() || !title.getString().startsWith(prefix)) continue;
                    String titleKey = data.has("titleKey") ? data.get("titleKey").getAsString() : null;
                    String path = data.get("texture").getAsString();
                    if (!path.startsWith("textures/") || path.contains("..")) continue;
                    var texture = Identifier.tryBuild(entry.getKey().getNamespace(), path);
                    int width = data.get("width").getAsInt(), height = data.get("height").getAsInt();
                    int textureHeight = data.get("textureHeight").getAsInt(), fillV = data.get("fillV").getAsInt();
                    float smoothing = data.get("smoothing").getAsFloat();
                    if (texture != null && width > 0 && width <= 512 && height > 0 && height <= 32
                            && textureHeight >= height && textureHeight <= 512 && fillV >= 0
                            && fillV <= textureHeight - height && smoothing > 0 && smoothing <= 1
                            && manager.getResource(texture).isPresent())
                        return new Style(prefix, titleKey, texture, width, height, textureHeight, fillV, smoothing);
                }
            } catch (Exception ignored) {}
        }
        return null;
    }

    public static float advance(float value, float target, float smoothing, long ticks) {
        return target + (value - target) * (float)Math.pow(1 - smoothing, Math.max(0, ticks));
    }

    public static void render(Minecraft client, GuiGraphicsExtractor graphics, Style style, Component title, float progress) {
        int x = (graphics.guiWidth() - style.width()) / 2;
        if (style.titleKey() != null && title.getString().startsWith(style.prefix()))
            title = Component.translatableWithFallback(style.titleKey(), style.prefix() + "%s",
                    title.getString().substring(style.prefix().length()));
        graphics.text(client.font, title, (graphics.guiWidth() - client.font.width(title)) / 2, 12, 0xffffffff, true);
        graphics.blit(RenderPipelines.GUI_TEXTURED, style.texture(), x, 24, 0, 0,
                style.width(), style.height(), style.width(), style.textureHeight(), 0xffffffff);
        int fill = Math.clamp(Math.round(style.width() * progress), 0, style.width());
        if (fill > 0) graphics.blit(RenderPipelines.GUI_TEXTURED, style.texture(), x, 24, 0, style.fillV(),
                fill, style.height(), style.width(), style.textureHeight(), 0xffffffff);
    }
}
