package dev.moddedmc.companion;

import com.google.gson.Gson;
import com.google.gson.JsonParser;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;
import net.minecraft.client.Minecraft;
import net.minecraft.resources.Identifier;

/** Generic standalone layout; all geometry and fields are declared by the pack. */
record StandaloneLayout(int version, String title, int width, int height, String texture,
                        int textureWidth, int textureHeight, int color, Map<String, Atlas> atlases,
                        List<Tab> tabs, List<Rows> lists, List<Label> labels, List<Layer> layers,
                        List<Control> controls, Input input, Pages pages, String refreshAction, int refreshInterval,
                        List<RigPreview> previews) {
    record Atlas(String texture, int width, int height) {}
    record Tab(String key, String active, String inactive, int x, int y, int width, int height) {}
    record Rows(String source, String tab, int x, int y, int width, int count, int spacing,
                String field, boolean select, String empty) {}
    record Label(String tab, String field, boolean selected, int x, int y, int width, int lines, boolean center) {}
    record Layer(String tab, String atlas, int x, int y, boolean selected, String field, String equals,
                 List<String> frames, int frameTicks) {}
    record RigPreview(String tab, String field, int x, int y, int width, int height) {}
    record Control(String tab, String atlas, int x, int y, String label, String action,
                   boolean selected, boolean input, String confirm) {}
    record Input(String tab, int x, int y, int width, int height, int maxLength) {}
    record Pages(String tab, String field, int x, int y, int width, int lines, int spacing, int navX, int navY,
                 String sections, int sectionX, int sectionY, int sectionWidth, String itemField) {}

    static StandaloneLayout parse(String json, String id) {
        try {
            var entry = JsonParser.parseString(json).getAsJsonObject().get(id);
            var layout = new Gson().fromJson(entry, StandaloneLayout.class);
            return layout != null && layout.valid() ? layout : null;
        } catch (RuntimeException ignored) { return null; }
    }

    static StandaloneLayout load(Minecraft client, Identifier id) {
        var manager = client.getResourceManager();
        var catalog = manager.getResource(Identifier.fromNamespaceAndPath(id.getNamespace(), "moddedmc/screens.json"));
        if (catalog.isEmpty()) return null;
        try (var stream = catalog.get().open()) {
            var bytes = stream.readNBytes(256 * 1024 + 1);
            if (bytes.length > 256 * 1024) return null;
            var layout = parse(new String(bytes, StandardCharsets.UTF_8), id.getPath());
            if (layout == null || manager.getResource(Identifier.fromNamespaceAndPath(id.getNamespace(), layout.texture)).isEmpty()) return null;
            for (var atlas : layout.atlases.values()) {
                if (manager.getResource(Identifier.fromNamespaceAndPath(id.getNamespace(), atlas.texture)).isEmpty()) return null;
            }
            return layout;
        } catch (Exception ignored) { return null; }
    }

    private boolean valid() {
        if (version != 2 || title == null || width < 1 || width > 512 || height < 1 || height > 512
                || textureWidth < width || textureHeight < height || textureWidth > 4096 || textureHeight > 4096
                || !texture(texture) || atlases == null || atlases.size() > 64 || tabs == null || tabs.isEmpty() || tabs.size() > 16
                || lists == null || lists.size() > 16 || labels == null || labels.size() > 64
                || layers == null || layers.size() > 64 || controls == null || controls.size() > 64) return false;
        for (var atlas : atlases.values()) if (!texture(atlas.texture) || atlas.width < 1 || atlas.width > 512 || atlas.height < 1 || atlas.height > 512) return false;
        var keys = new java.util.HashSet<String>();
        for (var tab : tabs) if (!key(tab.key) || !keys.add(tab.key) || !atlases.containsKey(tab.active) || !atlases.containsKey(tab.inactive)
                || !fits(tab.x, tab.y, tab.width, tab.height) || !atlasFits(tab.active, tab.x, tab.y) || !atlasFits(tab.inactive, tab.x, tab.y)) return false;
        for (var rows : lists) if (!key(rows.source) || !optionalKey(rows.field) || !tab(rows.tab)
                || rows.count < 1 || rows.count > 64 || rows.spacing < 1 || rows.spacing > 64 || !fits(rows.x, rows.y, rows.width, rows.count * rows.spacing)) return false;
        for (var label : labels) if (!key(label.field) || !tab(label.tab) || label.lines < 1 || label.lines > 32 || !fits(label.x, label.y, label.width, label.lines * 9)) return false;
        for (var layer : layers) {
            if (!tab(layer.tab) || !atlasFits(layer.atlas, layer.x, layer.y) || !optionalKey(layer.field)) return false;
            if (layer.frames != null) {
                if (layer.frames.isEmpty() || layer.frames.size() > 16 || layer.frameTicks < 1 || layer.frameTicks > 1200) return false;
                for (String frame : layer.frames) if (!atlasFits(frame, layer.x, layer.y)) return false;
            }
        }
        if (previews != null) {
            if (previews.size() > 8) return false;
            for (var preview : previews) if (!tab(preview.tab) || !key(preview.field)
                    || !fits(preview.x, preview.y, preview.width, preview.height)) return false;
        }
        for (var control : controls) if (!tab(control.tab) || !atlasFits(control.atlas, control.x, control.y) || !key(control.action)
                || control.label == null || control.input && input == null) return false;
        if (input != null && (!tab(input.tab) || input.maxLength < 1 || input.maxLength > 64 || !fits(input.x, input.y, input.width, input.height))) return false;
        if (refreshAction != null && (!key(refreshAction) || refreshInterval < 10 || refreshInterval > 1200)) return false;
        return pages == null || tab(pages.tab) && key(pages.field) && pages.lines > 0 && pages.lines <= 32 && pages.spacing > 0 && pages.spacing <= 32
                && fits(pages.x, pages.y, pages.width, pages.lines * pages.spacing) && fits(pages.navX - 25, pages.navY, 50, 10)
                && optionalKey(pages.itemField)
                && (pages.sections == null || key(pages.sections) && fits(pages.sectionX, pages.sectionY, pages.sectionWidth, 30));
    }
    private boolean tab(String key) { return key == null || tabs.stream().anyMatch(t -> t.key.equals(key)); }
    private boolean atlasFits(String name, int x, int y) {
        var a = atlases.get(name);
        return a != null && fits(x, y, a.width, a.height);
    }
    private boolean fits(int x, int y, int w, int h) { return x >= 0 && y >= 0 && w > 0 && h > 0 && x <= width - w && y <= height - h; }
    private static boolean texture(String value) { return value != null && !value.contains("..") && value.matches("textures/([a-z0-9_.-]+/)*[a-z0-9_.-]+[.]png"); }
    private static boolean key(String value) { return value != null && value.matches("[a-z0-9_.-]{1,64}"); }
    private static boolean optionalKey(String value) { return value == null || key(value); }
}
