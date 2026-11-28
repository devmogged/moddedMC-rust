package dev.moddedmc.companion;

import java.nio.charset.StandardCharsets;
import java.io.IOException;
import java.util.HashMap;
import dev.moddedmc.companion.rig.DisplayRigs;
import dev.moddedmc.companion.rig.GuiRigState;
import net.fabricmc.loader.api.FabricLoader;
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
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.entity.ai.attributes.AttributeModifier;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.inventory.Slot;
import net.minecraft.world.item.ItemStack;
import org.joml.Quaternionf;
import org.joml.Vector3f;

/** Repositions a vanilla chest menu; packet slot ids and normal inventory prediction remain intact. */
public final class PackScreen extends AbstractContainerScreen<ChestMenu> {
    public static final String PREFIX = "moddedmc:screen/";
    private final ScreenLayout layout;
    private final Identifier texture;
    private DisplayRigs.Preview preview;

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
        if (layout.preview() != null) extractPreview(graphics, state, mx, my, partialTick);
    }

    private void extractPreview(GuiGraphicsExtractor graphics, CompoundTag state, int mx, int my, float delta) {
        var definition = layout.preview();
        String uuid = state.getStringOr("uuid", "");
        if (uuid.isEmpty() || minecraft.level == null) return;
        var entity = minecraft.level.getEntity(state.getIntOr("entity", -1));
        if (!(entity instanceof LivingEntity living) || !living.getUUID().toString().replace("-", "").equals(uuid)) return;
        if (FabricLoader.getInstance().isModLoaded("geckolib")) {
            if (preview == null) preview = new DisplayRigs.Preview();
            var frame = preview.extract(Identifier.parse(definition.model()), "", uuid, delta);
            if (frame != null) {
                int x = leftPos + definition.x(), y = topPos + definition.y();
                float yaw = (float)Math.atan((x + definition.width() / 2f - mx) / 40f);
                float pitch = (float)Math.atan((y + definition.height() / 2f - my) / 40f);
                var rotation = new Quaternionf().rotateZ((float)Math.PI)
                        .rotateX(pitch * (float)Math.toRadians(20)).rotateY(-yaw * (float)Math.toRadians(20));
                graphics.enableScissor(x, y, x + definition.width(), y + definition.height());
                try {
                    graphics.entity(new GuiRigState(frame), definition.scale(),
                            new Vector3f(0, definition.height() / (2f * definition.scale()), 0),
                            rotation, new Quaternionf(), x, y, x + definition.width(), y + definition.height());
                } finally { graphics.disableScissor(); }
            }
        }
        if (layout.vitals() == null) return;
        for (var vital : layout.vitals()) {
            boolean health = vital.kind().equals("health");
            float value = health ? living.getHealth() : armor();
            float maximum = health ? living.getMaxHealth() : 20;
            int fill = Math.round(44 * Math.clamp(value / Math.max(maximum, 1), 0, 1));
            int x = leftPos + vital.x(), y = topPos + vital.y();
            blit(graphics, x, y, vital.iconU(), vital.iconV(), 9, 9, 0xffffffff);
            blit(graphics, x + 12, y, vital.barU(), vital.barV(), 62, 8, 0xffffffff);
            if (fill > 0) blit(graphics, x + 15, y + 3, vital.fillU(), vital.fillV(), fill, 2, 0xffffffff);
            String label = Integer.toString(Math.round(value));
            graphics.text(font, label, leftPos + vital.valueX() - font.width(label), y, 0xff000000, false);
        }
    }

    private float armor() {
        EquipmentSlot[] slots = {EquipmentSlot.HEAD, EquipmentSlot.CHEST, EquipmentSlot.LEGS, EquipmentSlot.FEET};
        double[] total = {0};
        for (int i = 0; i < slots.length; i++) menu.getSlot(i).getItem().forEachModifier(slots[i], (attribute, modifier) -> {
            if (attribute.equals(Attributes.ARMOR) && modifier.operation() == AttributeModifier.Operation.ADD_VALUE)
                total[0] += modifier.amount();
        });
        return (float)total[0];
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
