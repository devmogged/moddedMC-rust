package dev.moddedmc.companion;

import com.google.gson.Gson;
import com.google.gson.JsonParser;
import java.util.HashSet;
import java.util.List;
import java.util.Set;

/** Versioned, bounded data supplied by the resource pack. No mod-specific code. */
public record ScreenLayout(int version, String title, int width, int height, String texture,
                           int textureWidth, int textureHeight, int containerSlots, int stateSlot,
                           List<Position> slots, Inventory inventory, List<Layer> layers,
                           List<Button> buttons, List<Label> labels, RigPreview preview, List<Vital> vitals) {
    public record Position(int index, int x, int y, String hideWhen) {}
    public record Inventory(int x, int y, int hotbarY) {}
    public record Layer(int x, int y, int u, int v, int width, int height,
                        String fill, String visibleWhen) {}
    public record Button(int slot, int x, int y, int u, int v, int hoverV, int pressedV,
                         int width, int height, String enabledWhen, String label) {}
    public record Label(int x, int y, int color, String text) {}
    public record RigPreview(String model, int x, int y, int width, int height, int scale) {}
    public record Vital(String kind, int x, int y, int iconU, int iconV, int barU, int barV,
                        int fillU, int fillV, int valueX) {}

    public static ScreenLayout parse(String json, String id) {
        try {
            var entry = JsonParser.parseString(json).getAsJsonObject().get(id);
            var layout = new Gson().fromJson(entry, ScreenLayout.class);
            return layout != null && layout.valid() ? layout : null;
        } catch (RuntimeException ignored) {
            return null;
        }
    }

    private boolean valid() {
        if (version != 1 || title == null || title.length() > 256
                || width < 1 || width > 512 || height < 1 || height > 512
                || textureWidth < width || textureWidth > 4096
                || textureHeight < height || textureHeight > 4096
                || texture == null || !texture.matches("textures/([a-z0-9_.-]+/)*[a-z0-9_.-]+[.]png")
                || texture.contains("..") || containerSlots < 9 || containerSlots > 54
                || containerSlots % 9 != 0 || stateSlot < 0 || stateSlot >= containerSlots
                || slots == null || inventory == null || layers == null || buttons == null || labels == null
                || slots.size() > 54 || layers.size() > 64 || buttons.size() > 54 || labels.size() > 64) return false;
        Set<Integer> used = new HashSet<>();
        for (var slot : slots) {
            if (slot.index < 0 || slot.index >= containerSlots || slot.index == stateSlot
                    || !used.add(slot.index) || !fits(slot.x, slot.y, 16, 16, width, height)
                    || !key(slot.hideWhen)) return false;
        }
        if (!fits(inventory.x, inventory.y, 160, 52, width, height)
                || !fits(inventory.x, inventory.hotbarY, 160, 16, width, height)) return false;
        for (var layer : layers) {
            if (!fits(layer.x, layer.y, layer.width, layer.height, width, height)
                    || !fits(layer.u, layer.v, layer.width, layer.height, textureWidth, textureHeight)
                    || !key(layer.fill) || !key(layer.visibleWhen)) return false;
        }
        for (var button : buttons) {
            if (button.slot < 0 || button.slot >= containerSlots || !used.add(button.slot)
                    || !fits(button.x, button.y, button.width, button.height, width, height)
                    || !fits(button.u, button.v, button.width, button.height, textureWidth, textureHeight)
                    || !fits(button.u, button.hoverV, button.width, button.height, textureWidth, textureHeight)
                    || !fits(button.u, button.pressedV, button.width, button.height, textureWidth, textureHeight)
                    || !key(button.enabledWhen) || button.label == null || button.label.length() > 256) return false;
        }
        for (var label : labels) {
            if (!fits(label.x, label.y, 1, 1, width, height)
                    || label.text == null || label.text.length() > 256) return false;
        }
        if (preview != null && (preview.model == null
                || !preview.model.matches("[a-z0-9_.-]+:[a-z0-9_./-]+")
                || preview.scale < 1 || preview.scale > 128
                || !fits(preview.x, preview.y, preview.width, preview.height, width, height))) return false;
        if (vitals != null) {
            if (preview == null || vitals.size() > 2) return false;
            for (var vital : vitals) {
                if (!Set.of("health", "armor").contains(vital.kind)
                        || !fits(vital.x, vital.y, 74, 9, width, height)
                        || !fits(vital.iconU, vital.iconV, 9, 9, textureWidth, textureHeight)
                        || !fits(vital.barU, vital.barV, 62, 8, textureWidth, textureHeight)
                        || !fits(vital.fillU, vital.fillV, 44, 2, textureWidth, textureHeight)
                        || !fits(vital.valueX, vital.y, 1, 9, width, height)) return false;
            }
        }
        return true;
    }

    private static boolean key(String key) {
        return key == null || key.matches("[a-z0-9_.-]{1,64}");
    }

    private static boolean fits(int x, int y, int w, int h, int maxW, int maxH) {
        return x >= 0 && y >= 0 && w > 0 && h > 0 && w <= maxW && h <= maxH
                && x <= maxW - w && y <= maxH - h;
    }
}
