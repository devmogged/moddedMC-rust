package dev.moddedmc.companion;

import java.nio.charset.StandardCharsets;
import java.io.IOException;
import java.util.HashMap;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.AbstractWidget;
import net.minecraft.client.gui.narration.NarrationElementOutput;
import net.minecraft.client.gui.screens.inventory.AbstractContainerScreen;
import net.minecraft.client.input.MouseButtonEvent;
import net.minecraft.client.renderer.RenderPipelines;
import net.minecraft.core.component.DataComponents;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.inventory.Slot;
import net.minecraft.world.item.ItemStack;

/** Repositions a vanilla chest menu; packet slot ids and normal inventory prediction remain intact. */
public final class PackScreen extends AbstractContainerScreen<ChestMenu> {
    public static final String PREFIX = "moddedmc:screen/";
    private final ScreenLayout layout;
    private final Identifier texture;

    public static ScreenLayout load(Minecraft client, Identifier id) {
        var manager = client.getResourceManager();
        var catalog = manager.getResource(Identifier.fromNamespaceAndPath(id.getNamespace(), "moddedmc/screens.json"));
        if (catalog.isEmpty()) return null;
        try (var stream = catalog.get().open()) {
            var bytes = stream.readNBytes(256 * 1024 + 1);
            if (bytes.length > 256 * 1024) return null;
            var layout = ScreenLayout.parse(new String(bytes, StandardCharsets.UTF_8), id.getPath());
            if (layout == null || manager.getResource(Identifier.fromNamespaceAndPath(id.getNamespace(), layout.texture())).isEmpty()) return null;
            return layout;
        } catch (IOException | RuntimeException ignored) {
            return null;
        }
    }

    public PackScreen(ChestMenu menu, Inventory inventory, Identifier id, ScreenLayout layout) {
        super(menu, inventory, Component.literal(layout.title()), layout.width(), layout.height());
        this.layout = layout;
        this.texture = Identifier.fromNamespaceAndPath(id.getNamespace(), layout.texture());
        var positions = new HashMap<Integer, ScreenLayout.Position>();
        for (var position : layout.slots()) positions.put(position.index(), position);
        int count = layout.containerSlots();
        for (int i = 0; i < menu.slots.size(); i++) {
            var old = menu.slots.get(i);
            var position = positions.get(i);
            int x = -1000, y = -1000;
            if (position != null) {
                x = position.x();
                y = position.y();
            } else if (i >= count) {
                int playerSlot = i - count;
                x = layout.inventory().x() + playerSlot % 9 * 18;
                y = playerSlot < 27 ? layout.inventory().y() + playerSlot / 9 * 18 : layout.inventory().hotbarY();
            }
            final boolean hidden = i < count && position == null;
            var slot = new Slot(old.container, old.getContainerSlot(), x, y) {
                @Override public boolean isActive() {
                    return !hidden && (position == null || !flag(position.hideWhen()));
                }
                @Override public boolean mayPlace(ItemStack stack) { return isActive() && old.mayPlace(stack); }
                @Override public boolean mayPickup(Player player) { return isActive() && old.mayPickup(player); }
            };
            slot.index = old.index;
            menu.slots.set(i, slot);
        }
    }

    private CompoundTag state() {
        var data = menu.slots.get(layout.stateSlot()).getItem().get(DataComponents.CUSTOM_DATA);
        return data == null ? new CompoundTag() : data.copyTag().getCompoundOrEmpty("moddedmc:screen_state");
    }

    private boolean flag(String key) { return key != null && state().getIntOr(key, 0) != 0; }

    private boolean enabled(ScreenLayout.Button button) {
        return (button.enabledWhen() == null || flag(button.enabledWhen())) && menu.getCarried().isEmpty();
    }

    @Override protected void init() {
        super.init();
        for (var button : layout.buttons()) addRenderableWidget(new TextureButton(button));
    }

    @Override public void extractBackground(GuiGraphicsExtractor graphics, int mx, int my, float partialTick) {
        super.extractBackground(graphics, mx, my, partialTick);
        blit(graphics, leftPos, topPos, 0, 0, layout.width(), layout.height(), 0xffffffff);
        var state = state();
        for (var layer : layout.layers()) {
            if (layer.visibleWhen() != null && state.getIntOr(layer.visibleWhen(), 0) == 0) continue;
            int fill = layer.fill() == null ? layer.height() : Math.clamp(state.getIntOr(layer.fill(), 0), 0, layer.height());
            int offset = layer.height() - fill;
            if (fill != 0) blit(graphics, leftPos + layer.x(), topPos + layer.y() + offset,
                    layer.u(), layer.v() + offset, layer.width(), fill, 0xffffffff);
        }
    }

    @Override protected void extractLabels(GuiGraphicsExtractor graphics, int mx, int my) {
        for (var label : layout.labels()) graphics.text(font, Component.literal(label.text()),
                label.x(), label.y(), label.color(), false);
    }

    private void blit(GuiGraphicsExtractor graphics, int x, int y, int u, int v, int w, int h, int color) {
        graphics.blit(RenderPipelines.GUI_TEXTURED, texture, x, y, u, v, w, h,
                layout.textureWidth(), layout.textureHeight(), color);
    }

    private final class TextureButton extends AbstractWidget {
        private final ScreenLayout.Button definition;
        private boolean pressed;

        TextureButton(ScreenLayout.Button definition) {
            super(leftPos + definition.x(), topPos + definition.y(), definition.width(), definition.height(),
                    Component.literal(definition.label()));
            this.definition = definition;
        }

        @Override protected void extractWidgetRenderState(GuiGraphicsExtractor graphics, int mx, int my, float partialTick) {
            active = enabled(definition);
            int v = !active ? definition.v() : pressed ? definition.pressedV() : isHovered() ? definition.hoverV() : definition.v();
            blit(graphics, getX(), getY(), definition.u(), v, getWidth(), getHeight(), active ? 0xffffffff : 0xff737373);
        }

        @Override public boolean mouseClicked(MouseButtonEvent event, boolean doubleClick) {
            active = enabled(definition);
            return super.mouseClicked(event, doubleClick);
        }

        @Override public void onClick(MouseButtonEvent event, boolean doubleClick) {
            if (!enabled(definition) || minecraft.gameMode == null || minecraft.player == null) return;
            pressed = true;
            minecraft.gameMode.handleContainerInput(menu.containerId, definition.slot(), 0, ContainerInput.PICKUP, minecraft.player);
        }

        @Override public void onRelease(MouseButtonEvent event) { pressed = false; }
        @Override protected void updateWidgetNarration(NarrationElementOutput output) { defaultButtonNarrationText(output); }
    }
}
