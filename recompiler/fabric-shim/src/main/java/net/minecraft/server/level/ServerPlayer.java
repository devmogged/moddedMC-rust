package net.minecraft.server.level;

import java.util.Set;

import com.mojang.authlib.GameProfile;

import moddedmc.rt.Host;
import net.minecraft.network.chat.Component;
import net.minecraft.world.entity.Relative;

/** shortcut: a view over a host handle valid for one command only; mods that keep players need stable handles. */
public class ServerPlayer {
    private final int handle;

    public ServerPlayer(int handle) {
        this.handle = handle;
    }

    public int handle() {
        return handle;
    }

    public GameProfile getGameProfile() {
        return new GameProfile(null, Host.text(Host.TEXT_PLAYER_NAME, handle));
    }

    public ServerLevel level() {
        return new ServerLevel(Host.text(Host.TEXT_PLAYER_DIMENSION, handle));
    }

    public double getX() {
        return Host.playerNum(handle, Host.NUM_X);
    }

    public double getY() {
        return Host.playerNum(handle, Host.NUM_Y);
    }

    public double getZ() {
        return Host.playerNum(handle, Host.NUM_Z);
    }

    public float getYRot() {
        return (float) Host.playerNum(handle, Host.NUM_YAW);
    }

    public float getXRot() {
        return (float) Host.playerNum(handle, Host.NUM_PITCH);
    }

    public void sendSystemMessage(Component message) {
        Host.sendMessage(handle, message.getString());
    }

    /** shortcut: absolute moves only; relative flags are ignored until a corpus mod passes some. */
    public boolean teleportTo(ServerLevel level, double x, double y, double z, Set<Relative> relatives, float yaw,
            float pitch, boolean resetCamera) {
        Host.teleport(handle, level.dimension().identifier().toString(), x, y, z, yaw, pitch);
        return true;
    }
}
