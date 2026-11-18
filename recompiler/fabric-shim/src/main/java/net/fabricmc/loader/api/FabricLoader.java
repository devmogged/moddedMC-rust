package net.fabricmc.loader.api;

import java.nio.file.Path;
import java.nio.file.Paths;

import moddedmc.rt.Host;

public interface FabricLoader {
    static FabricLoader getInstance() {
        return Instance.LOADER;
    }

    /** The plugin data folder in Pumpkin, the one directory the mod may write to. */
    default Path getConfigDir() {
        return Paths.get(Host.text(Host.TEXT_CONFIG_DIR, 0));
    }

    final class Instance {
        static final FabricLoader LOADER = new FabricLoader() {};

        private Instance() {}
    }
}
