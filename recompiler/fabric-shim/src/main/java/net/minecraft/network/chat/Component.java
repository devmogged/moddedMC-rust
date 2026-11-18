package net.minecraft.network.chat;

public interface Component {
    static MutableComponent literal(String text) {
        return new MutableComponent(text);
    }

    String getString();
}
