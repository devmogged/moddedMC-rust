package dev.moddedmc.companion.mixin;

import dev.moddedmc.companion.PackScreen;
import dev.moddedmc.companion.Companion;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.screens.MenuScreens;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.inventory.MenuType;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(MenuScreens.class)
public abstract class MenuScreensMixin {
    @Inject(method = "create", at = @At("HEAD"), cancellable = true)
    private static void moddedmc$create(MenuType<?> type, Minecraft client, int syncId, Component title, CallbackInfo ci) {
        String name = title.getString();
        if (!Companion.negotiated() || !name.startsWith(PackScreen.PREFIX) || client.player == null) return;
        if (type != MenuType.GENERIC_9x1 && type != MenuType.GENERIC_9x2 && type != MenuType.GENERIC_9x3
                && type != MenuType.GENERIC_9x4 && type != MenuType.GENERIC_9x5 && type != MenuType.GENERIC_9x6) return;
        var id = Identifier.tryParse(name.substring(PackScreen.PREFIX.length()));
        var inventory = client.player.getInventory();
        var menu = (ChestMenu) type.create(syncId, inventory);
        var layout = id == null ? null : PackScreen.load(client, id);
        client.player.containerMenu = menu;
        ci.cancel();
        if (layout == null || menu.getRowCount() * 9 != layout.containerSlots()) {
            client.player.closeContainer();
            client.player.sendSystemMessage(Component.literal("Missing or invalid moddedMC screen: " + name));
            return;
        }
        client.gui.setScreen(new PackScreen(menu, inventory, id, layout));
    }
}
