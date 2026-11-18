package net.minecraft.world.level;

import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;

public class Level {
    private final ResourceKey<Level> dimension;

    protected Level(String dimension) {
        this.dimension = new ResourceKey<>(Identifier.parse(dimension));
    }

    public ResourceKey<Level> dimension() {
        return dimension;
    }
}
