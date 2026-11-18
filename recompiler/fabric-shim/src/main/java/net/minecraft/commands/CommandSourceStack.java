package net.minecraft.commands;

import com.mojang.brigadier.LiteralMessage;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.exceptions.SimpleCommandExceptionType;

import moddedmc.rt.Host;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;

public class CommandSourceStack {
    static final SimpleCommandExceptionType ERROR_NOT_PLAYER =
            new SimpleCommandExceptionType(new LiteralMessage("A player is required to run this command here"));

    private final ServerPlayer player;
    private final MinecraftServer server;

    public CommandSourceStack(ServerPlayer player, MinecraftServer server) {
        this.player = player;
        this.server = server;
    }

    public ServerPlayer getPlayerOrException() throws CommandSyntaxException {
        if (player == null) {
            throw ERROR_NOT_PLAYER.create();
        }
        return player;
    }

    public ServerPlayer getPlayer() {
        return player;
    }

    public MinecraftServer getServer() {
        return server;
    }

    /** Handle 0 reaches whoever ran the command, the console included. */
    public void sendFailure(String message) {
        Host.sendMessage(player == null ? 0 : player.handle(), message);
    }
}
