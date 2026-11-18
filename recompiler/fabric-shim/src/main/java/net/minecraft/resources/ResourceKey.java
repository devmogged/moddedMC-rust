package net.minecraft.resources;

public final class ResourceKey<T> {
    private final Identifier identifier;

    public ResourceKey(Identifier identifier) {
        this.identifier = identifier;
    }

    public Identifier identifier() {
        return identifier;
    }

    @Override
    public String toString() {
        return "ResourceKey[" + identifier + "]";
    }
}
