package dev.moddedmc.companion;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import dev.moddedmc.companion.rig.DisplayRigs;
import dev.moddedmc.companion.rig.GuiRigState;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.EditBox;
import net.minecraft.client.gui.screens.ConfirmScreen;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.input.MouseButtonEvent;
import net.minecraft.client.renderer.RenderPipelines;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.util.FormattedCharSequence;
import net.minecraft.world.item.ItemStack;
import org.joml.Quaternionf;
import org.joml.Vector3f;

/** Pack-defined book/dialog with local selection, tabs, scrolling and server-bound controls. */
final class StandalonePackScreen extends Screen {
    private final Identifier id;
    private final StandaloneLayout layout;
    private final HashMap<String, Integer> offsets = new HashMap<>();
    private String token, tab, selected = "";
    private JsonObject state;
    private int left, top, page, ticks;
    private boolean waiting;
    private EditBox input;
    private int animationTicks;
    private final HashMap<StandaloneLayout.RigPreview, DisplayRigs.Preview> previews = new HashMap<>();
    private final HashMap<Identifier, ItemStack> catalogItems = new HashMap<>();
    private byte[] catalogBytes;

    StandalonePackScreen(Identifier id, StandaloneLayout layout, String token, JsonObject state) {
        super(Component.literal(layout.title()));
        this.id = id;
        this.layout = layout;
        this.tab = layout.tabs().getFirst().key();
        update(token, state);
    }

    boolean matches(Identifier other) { return id.equals(other); }
    boolean hasToken(String other) { return token.equals(other); }
    void update(String token, JsonObject state) {
        this.token = token;
        this.state = state;
        waiting = false;
        ticks = 0;
        if (selected.isEmpty()) selected = string(state, "selected");
        if (selectedEntry() == null) { selected = ""; page = 0; }
    }

    @Override protected void init() {
        left = Math.max(0, (width - layout.width()) / 2);
        top = Math.max(0, (height - layout.height()) / 2);
        var definition = layout.input();
        if (definition != null) {
            String value = input == null ? "" : input.getValue();
            input = addRenderableWidget(new EditBox(font, left + definition.x(), top + definition.y(),
                    definition.width(), definition.height(), Component.literal("Input")));
            input.setMaxLength(definition.maxLength());
            input.setBordered(false);
            input.setTextShadow(false);
            input.setTextColor(layout.color());
            input.setTextColorUneditable(layout.color());
            input.setValue(value);
            inputVisibility();
        }
    }

    private void inputVisibility() {
        if (input == null) return;
        boolean visible = visible(layout.input().tab());
        input.setVisible(visible);
        input.setEditable(visible);
        if (!visible) input.setFocused(false);
    }

    @Override public boolean isPauseScreen() { return false; }
    @Override public void tick() {
        animationTicks++;
        if (waiting || layout.refreshAction() == null || ++ticks < layout.refreshInterval()) return;
        String action = string(state, layout.refreshAction());
        if (!action.isEmpty()) {
            waiting = true;
            PackScreens.click(token, action, "");
        }
    }
    private boolean visible(String key) { return key == null || tab.equals(key); }
    private static String string(JsonElement object, String field) {
        if (object == null || field == null || !object.isJsonObject()) return "";
        var value = object.getAsJsonObject().get(field);
        return value != null && value.isJsonPrimitive() ? value.getAsString() : "";
    }
    private static int number(JsonElement object, String field, int fallback) {
        try { return Integer.parseInt(string(object, field)); }
        catch (NumberFormatException ignored) { return fallback; }
    }

    private void preview(GuiGraphicsExtractor graphics, StandaloneLayout.RigPreview definition,
                         JsonElement entry, int mx, int my, float delta) {
        if (!FabricLoader.getInstance().isModLoaded("geckolib") || entry == null || !entry.isJsonObject()) return;
        var data = entry.getAsJsonObject().get(definition.field());
        var model = Identifier.tryParse(string(data, "rig"));
        int scale = number(data, "scale", 0);
        int dx = number(data, "offsetX", 0), dy = number(data, "offsetY", 0);
        if (model == null || scale < 1 || scale > 128 || Math.abs((long) dx) > definition.width()
                || Math.abs((long) dy) > definition.height()) return;
        var frame = previews.computeIfAbsent(definition, unused -> new DisplayRigs.Preview()).extract(
                model, string(data, "texture"), string(entry, "id"), delta);
        if (frame == null) return;
        int x = left + definition.x(), y = top + definition.y();
        int centerX = x + definition.width() / 2 + dx, centerY = y + definition.height() + dy;
        float yaw = (float) Math.atan((centerX - mx) / 40f);
        float pitch = (float) Math.atan((centerY - definition.height() - my) / 40f);
        var rotation = new Quaternionf().rotateZ((float) Math.PI)
                .rotateX(pitch * (float) Math.toRadians(20)).rotateY(-yaw * (float) Math.toRadians(20));
        var translation = new Vector3f(dx / (float) scale, (definition.height() / 2f + dy) / scale, 0);
        graphics.enableScissor(x, y, x + definition.width(), y + definition.height());
        try {
            graphics.entity(new GuiRigState(frame), scale, translation, rotation, new Quaternionf(),
                    x, y, x + definition.width(), y + definition.height());
        } finally { graphics.disableScissor(); }
    }

    private ItemStack icon(String value) {
        var id = Identifier.tryParse(value);
        if (id == null) return ItemStack.EMPTY;
        var bytes = Companion.creativeCatalog();
        if (bytes != catalogBytes) {
            catalogBytes = bytes;
            catalogItems.clear();
            if (bytes != null && minecraft.level != null) {
                for (var stack : CreativeCatalog.decode(bytes, minecraft.level.registryAccess())) {
                    var model = stack.get(DataComponents.ITEM_MODEL);
                    if (model != null) catalogItems.put(model, stack);
                }
            }
        }
        var custom = catalogItems.get(id);
        if (custom != null) return custom;
        return BuiltInRegistries.ITEM.getOptional(id).map(ItemStack::new).orElse(ItemStack.EMPTY);
    }
    private JsonArray rows(String source) {
        var value = state.get(source);
        return value != null && value.isJsonArray() ? value.getAsJsonArray() : new JsonArray();
    }
    private JsonElement selectedEntry() {
        if (selected.isEmpty()) return null;
        for (var definition : layout.lists()) if (definition.select()) {
            for (var row : rows(definition.source())) if (selected.equals(string(row, "id"))) return row;
        }
        return null;
    }
    private static boolean hit(double x, double y, int rx, int ry, int w, int h) {
        return x >= rx && y >= ry && x < rx + w && y < ry + h;
    }
    private int offset(StandaloneLayout.Rows definition) {
        return Math.clamp(offsets.getOrDefault(definition.source(), 0), 0, Math.max(0, rows(definition.source()).size() - definition.count()));
    }
    private String rowText(JsonElement row, StandaloneLayout.Rows definition) {
        return definition.field() == null ? (row.isJsonPrimitive() ? row.getAsString() : "") : string(row, definition.field());
    }
    private void atlas(GuiGraphicsExtractor graphics, String name, int x, int y) {
        var a = layout.atlases().get(name);
        blit(graphics, a.texture(), x, y, a.width(), a.height(), a.width(), a.height());
    }
    private void blit(GuiGraphicsExtractor graphics, String texture, int x, int y, int w, int h, int tw, int th) {
        graphics.blit(RenderPipelines.GUI_TEXTURED, Identifier.fromNamespaceAndPath(id.getNamespace(), texture),
            left + x, top + y, 0, 0, w, h, tw, th, 0xffffffff);
    }
    private void text(GuiGraphicsExtractor graphics, String value, int x, int y, int color) {
        graphics.text(font, value, left + x, top + y, color, false);
    }
    private List<FormattedCharSequence> wrap(String value, int width) {
        var lines = new ArrayList<FormattedCharSequence>();
        for (String line : value.split("\\R", -1)) {
            if (line.isEmpty()) lines.add(FormattedCharSequence.EMPTY);
            else lines.addAll(font.split(Component.literal(line), width));
        }
        return lines;
    }
    private List<FormattedCharSequence> pageLines() {
        var p = layout.pages();
        return p == null ? List.of() : wrap(string(selectedEntry(), p.field()), p.width());
    }
    private int overviewPages(List<FormattedCharSequence> lines) {
        return Math.max(1, (lines.size() + layout.pages().lines() - 1) / layout.pages().lines());
    }
    private JsonArray sections() {
        var entry = selectedEntry();
        var p = layout.pages();
        if (entry == null || p == null || p.sections() == null) return new JsonArray();
        var value = entry.getAsJsonObject().get(p.sections());
        return value != null && value.isJsonArray() ? value.getAsJsonArray() : new JsonArray();
    }
    private int totalPages(List<FormattedCharSequence> lines) {
        return overviewPages(lines) + Math.min(2, sections().size());
    }

    @Override public void extractRenderState(GuiGraphicsExtractor graphics, int mx, int my, float tick) {
        extractBackground(graphics, mx, my, tick);
        blit(graphics, layout.texture(), 0, 0, layout.width(), layout.height(), layout.textureWidth(), layout.textureHeight());
        var entry = selectedEntry();
        int color = layout.color();
        for (var layer : layout.layers()) {
            if (!visible(layer.tab()) || layer.selected() && entry == null) continue;
            if (layer.field() != null && !string(layer.selected() ? entry : state, layer.field()).equals(layer.equals())) continue;
            var frames = layer.frames();
            String frame = frames == null ? layer.atlas() : frames.get((animationTicks / layer.frameTicks()) % frames.size());
            atlas(graphics, frame, layer.x(), layer.y());
        }
        for (var t : layout.tabs()) atlas(graphics, visible(t.key()) ? t.active() : t.inactive(), t.x(), t.y());
        for (var list : layout.lists()) if (visible(list.tab())) {
            var rows = rows(list.source());
            int start = offset(list);
            if (rows.isEmpty() && list.empty() != null) {
                var lines = wrap(list.empty(), list.width());
                for (int i = 0; i < Math.min(lines.size(), 2); i++) graphics.text(font, lines.get(i), left + list.x(), top + list.y() + i * 10, color, false);
            }
            for (int i = 0; i < list.count() && start + i < rows.size(); i++) {
                var row = rows.get(start + i);
                var value = font.plainSubstrByWidth(rowText(row, list), list.width());
                int ink = list.select() && selected.equals(string(row, "id")) ? 0xff8b0000 : color;
                text(graphics, value, list.x(), list.y() + i * list.spacing(), ink);
            }
            if (start > 0) text(graphics, "â†‘", list.x() + list.width() - 9, list.y(), color);
            if (start + list.count() < rows.size()) text(graphics, "â†“", list.x() + list.width() - 9, list.y() + (list.count() - 1) * list.spacing(), color);
        }
        for (var label : layout.labels()) if (visible(label.tab()) && (!label.selected() || entry != null)) {
            var lines = wrap(string(label.selected() ? entry : state, label.field()), label.width());
            for (int i = 0; i < Math.min(lines.size(), label.lines()); i++) {
                var line = lines.get(i);
                int x = label.x() + (label.center() ? Math.max(0, (label.width() - font.width(line)) / 2) : 0);
                graphics.text(font, line, left + x, top + label.y() + i * 9, color, false);
            }
        }
        var p = layout.pages();
        if (p != null && visible(p.tab()) && entry != null) {
            var lines = pageLines();
            int total = totalPages(lines);
            page = Math.clamp(page, 0, total - 1);
            int overview = overviewPages(lines);
            var sections = sections();
            if (page < overview) {
                for (int i = 0; i < p.lines() && page * p.lines() + i < lines.size(); i++)
                    graphics.text(font, lines.get(page * p.lines() + i), left + p.x(), top + p.y() + i * p.spacing(), color, false);
            } else {
                var section = sections.get(page - overview);
                text(graphics, string(section, "label"), p.x(), p.y(), color);
                var items = p.itemField() == null || !section.isJsonObject() ? null : section.getAsJsonObject().get(p.itemField());
                if (items != null && items.isJsonArray()) {
                    int count = Math.min(items.getAsJsonArray().size(), (p.lines() * p.spacing() - 10) / 18);
                    for (int i = 0; i < count; i++) {
                        var item = items.getAsJsonArray().get(i);
                        if (!item.isJsonPrimitive() || !item.getAsJsonPrimitive().isString()) continue;
                        var stack = icon(item.getAsString());
                        if (stack.isEmpty()) continue;
                        int x = p.x() + 2, y = p.y() + 10 + i * 18;
                        graphics.item(stack, left + x, top + y);
                        text(graphics, stack.getHoverName().getString(), x + 18, y + 4, color);
                    }
                } else {
                    var body = wrap(string(section, "body"), p.width());
                    for (int i = 0; i < Math.min(p.lines() - 1, body.size()); i++)
                        graphics.text(font, body.get(i), left + p.x(), top + p.y() + (i + 1) * p.spacing(), color, false);
                }
            }
            if (!sections.isEmpty()) {
                text(graphics, "1. Overview", p.sectionX(), p.sectionY(), page < overview ? 0xff8b0000 : color);
                for (int i = 0; i < Math.min(2, sections.size()); i++)
                    text(graphics, (i + 2) + ". " + string(sections.get(i), "label"), p.sectionX(), p.sectionY() + (i + 1) * 10,
                            page == overview + i ? 0xff8b0000 : color);
            }
            if (total > 1) {
                text(graphics, (page + 1) + "/" + total, p.navX() - 7, p.navY(), color);
                if (page > 0) text(graphics, "<", p.navX() - 20, p.navY(), color);
                if (page + 1 < total) text(graphics, ">", p.navX() + 13, p.navY(), color);
            }
        }
        for (var control : layout.controls()) if (visible(control.tab()) && (!control.selected() || entry != null)) {
            atlas(graphics, control.atlas(), control.x(), control.y());
            var a = layout.atlases().get(control.atlas());
            if (hit(mx - left, my - top, control.x(), control.y(), a.width(), a.height()))
                graphics.setTooltipForNextFrame(Component.literal(control.label()), mx, my);
        }
        super.extractRenderState(graphics, mx, my, tick);
        if (layout.previews() != null) for (var definition : layout.previews())
            if (visible(definition.tab())) preview(graphics, definition, entry, mx, my, tick);
    }

    @Override public boolean mouseClicked(MouseButtonEvent event, boolean doubleClick) {
        if (event.button() != 0) return super.mouseClicked(event, doubleClick);
        double x = event.x() - left, y = event.y() - top;
        for (var t : layout.tabs()) if (hit(x, y, t.x(), t.y(), t.width(), t.height())) {
            tab = t.key();
            page = 0;
            inputVisibility();
            return true;
        }
        var entry = selectedEntry();
        for (var control : layout.controls()) if (visible(control.tab()) && (!control.selected() || entry != null)) {
            var a = layout.atlases().get(control.atlas());
            if (!hit(x, y, control.x(), control.y(), a.width(), a.height())) continue;
            String action = string(control.selected() ? entry : state, control.action());
            String value = control.input() && input != null ? input.getValue().trim() : "";
            if (waiting || action.isEmpty() || control.input() && value.isEmpty()) return true;
            Runnable send = () -> {
                waiting = true;
                PackScreens.click(token, action, value);
                if (control.input() && input != null) input.setValue("");
            };
            if (control.confirm() != null) {
                minecraft.gui.setScreen(new ConfirmScreen(yes -> { minecraft.gui.setScreen(this); if (yes) send.run(); },
                    Component.literal(control.label()), Component.literal(control.confirm())));
            } else send.run();
            return true;
        }
        var p = layout.pages();
        if (p != null && visible(p.tab()) && entry != null && !sections().isEmpty()
                && hit(x, y, p.sectionX(), p.sectionY(), p.sectionWidth(), (Math.min(2, sections().size()) + 1) * 10)) {
            int section = (int)(y - p.sectionY()) / 10;
            page = section == 0 ? 0 : overviewPages(pageLines()) + section - 1;
            return true;
        }
        if (p != null && visible(p.tab()) && entry != null && hit(x, y, p.navX() - 25, p.navY(), 50, 10)) {
            if (x < p.navX() - 10) page = Math.max(0, page - 1);
            if (x > p.navX() + 10) page = Math.min(totalPages(pageLines()) - 1, page + 1);
            return true;
        }
        for (var list : layout.lists()) if (list.select() && visible(list.tab()) && hit(x, y, list.x(), list.y(), list.width(), list.count() * list.spacing())) {
            var rows = rows(list.source());
            int index = offset(list) + (int)(y - list.y()) / list.spacing();
            if (index < rows.size()) { selected = string(rows.get(index), "id"); page = 0; return true; }
        }
        return super.mouseClicked(event, doubleClick);
    }

    @Override public boolean mouseScrolled(double mx, double my, double horizontal, double vertical) {
        if (vertical != 0) {
            for (var list : layout.lists()) if (visible(list.tab()) && hit(mx-left, my-top, list.x(), list.y(), list.width(), list.count()*list.spacing())) {
                offsets.put(list.source(), Math.clamp(offset(list) + (vertical < 0 ? 1 : -1), 0, Math.max(0, rows(list.source()).size()-list.count())));
                return true;
            }
        }
        return super.mouseScrolled(mx, my, horizontal, vertical);
    }
}
