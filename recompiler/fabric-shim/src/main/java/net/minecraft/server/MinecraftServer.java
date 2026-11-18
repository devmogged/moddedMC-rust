package net.minecraft.server;

import java.util.ArrayList;
import java.util.List;

import moddedmc.rt.Host;
import net.minecraft.server.level.ServerLevel;

public class MinecraftServer {
    public static final MinecraftServer INSTANCE = new MinecraftServer();

    public int getTickCount() {
        return Host.tickCount();
    }

    public Iterable<ServerLevel> getAllLevels() {
        List<ServerLevel> levels = new ArrayList<>();
        for (int i = 0, n = Host.worldCount(); i < n; i++) {
            levels.add(new ServerLevel(Host.text(Host.TEXT_WORLD_DIMENSION, i)));
        }
        return levels;
    }
}
