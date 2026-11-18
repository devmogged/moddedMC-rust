package net.minecraft.commands;

import com.mojang.brigadier.builder.LiteralArgumentBuilder;

public class Commands {
    public static LiteralArgumentBuilder<CommandSourceStack> literal(String name) {
        return LiteralArgumentBuilder.literal(name);
    }

    public enum CommandSelection { ALL, DEDICATED, INTEGRATED }
}
