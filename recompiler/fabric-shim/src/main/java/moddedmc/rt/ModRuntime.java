package moddedmc.rt;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.tree.CommandNode;

import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.minecraft.commands.CommandBuildContext;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;

import org.teavm.interop.Address;
import org.teavm.interop.Export;

/** Entry points the Rust host calls. main runs once, from the plugin's on_load. */
public final class ModRuntime {
    static final CommandDispatcher<CommandSourceStack> DISPATCHER = new CommandDispatcher<>();

    private ModRuntime() {}

    public static void main(String[] args) {
        // Keeps the exports reachable for TeaVM's dead-code elimination.
        if (Host.never() == 1) {
            runCommand(0, null, 0);
        }
        Entrypoints.init();
        CommandBuildContext context = new CommandBuildContext() {};
        for (CommandRegistrationCallback callback : CommandRegistrationCallback.EVENT.listeners()) {
            callback.register(DISPATCHER, context, Commands.CommandSelection.DEDICATED);
        }
        for (CommandNode<CommandSourceStack> node : DISPATCHER.getRoot().getChildren()) {
            Host.registerCommand(node.getName());
        }
    }

    /** Runs a command line (no slash). {@code player} is 0 for the console, else a host handle. */
    @Export(name = "mmc_run_command")
    public static int runCommand(int player, Address input, int length) {
        String command = Host.read(input, length);
        ServerPlayer sender = player == 0 ? null : new ServerPlayer(player);
        CommandSourceStack source = new CommandSourceStack(sender, MinecraftServer.INSTANCE);
        try {
            return DISPATCHER.execute(command, source);
        } catch (CommandSyntaxException e) {
            source.sendFailure(e.getMessage());
            return 0;
        } catch (RuntimeException e) {
            // Vanilla reports unexpected command exceptions to the sender instead of crashing.
            source.sendFailure("An unexpected error occurred trying to execute that command: " + e);
            return 0;
        }
    }
}
