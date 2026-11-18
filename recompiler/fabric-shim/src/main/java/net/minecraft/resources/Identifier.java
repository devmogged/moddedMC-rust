package net.minecraft.resources;

public final class Identifier {
    private final String namespace;
    private final String path;

    private Identifier(String namespace, String path) {
        this.namespace = namespace;
        this.path = path;
    }

    public static Identifier parse(String id) {
        int colon = id.indexOf(':');
        return colon < 0 ? new Identifier("minecraft", id)
                : new Identifier(id.substring(0, colon), id.substring(colon + 1));
    }

    public String getNamespace() {
        return namespace;
    }

    public String getPath() {
        return path;
    }

    @Override
    public String toString() {
        return namespace + ":" + path;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof Identifier id && id.namespace.equals(namespace) && id.path.equals(path);
    }

    @Override
    public int hashCode() {
        return 31 * namespace.hashCode() + path.hashCode();
    }
}
