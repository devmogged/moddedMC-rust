package net.minecraft.network.chat;

/** shortcut: plain text only; styles and siblings come when a corpus mod uses them. */
public class MutableComponent implements Component {
    private final String text;

    MutableComponent(String text) {
        this.text = text;
    }

    @Override
    public String getString() {
        return text;
    }
}
