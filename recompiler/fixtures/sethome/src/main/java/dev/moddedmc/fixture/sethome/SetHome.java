package dev.moddedmc.fixture.sethome;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;

import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;

import com.mojang.brigadier.context.CommandContext;
import com.mojang.brigadier.exceptions.CommandSyntaxException;

/** /sethome and /home with a cooldown, homes saved to a text file. No mixins. */
public class SetHome implements ModInitializer {
	static final int COOLDOWN_TICKS = 200;

	record Home(String dimension, double x, double y, double z, float yaw, float pitch) {}

	final Map<String, Home> homes = new HashMap<>();
	final Map<String, Integer> lastUse = new HashMap<>();
	Path file;

	@Override
	public void onInitialize() {
		file = FabricLoader.getInstance().getConfigDir().resolve("sethome-homes.txt");
		load();
		CommandRegistrationCallback.EVENT.register((dispatcher, registryAccess, environment) -> {
			dispatcher.register(Commands.literal("sethome").executes(this::setHome));
			dispatcher.register(Commands.literal("home").executes(this::home));
		});
	}

	int setHome(CommandContext<CommandSourceStack> ctx) throws CommandSyntaxException {
		ServerPlayer player = ctx.getSource().getPlayerOrException();
		String dimension = player.level().dimension().identifier().toString();
		homes.put(player.getGameProfile().name(), new Home(dimension,
				player.getX(), player.getY(), player.getZ(), player.getYRot(), player.getXRot()));
		save();
		player.sendSystemMessage(Component.literal("Home set."));
		return 1;
	}

	int home(CommandContext<CommandSourceStack> ctx) throws CommandSyntaxException {
		ServerPlayer player = ctx.getSource().getPlayerOrException();
		String name = player.getGameProfile().name();
		Home home = homes.get(name);
		if (home == null) {
			player.sendSystemMessage(Component.literal("No home set."));
			return 0;
		}
		int now = ctx.getSource().getServer().getTickCount();
		Integer last = lastUse.get(name);
		if (last != null && now - last < COOLDOWN_TICKS) {
			int seconds = (COOLDOWN_TICKS - (now - last) + 19) / 20;
			player.sendSystemMessage(Component.literal("Home is on cooldown for " + seconds + "s."));
			return 0;
		}
		ServerLevel level = null;
		for (ServerLevel candidate : ctx.getSource().getServer().getAllLevels()) {
			if (candidate.dimension().identifier().toString().equals(home.dimension())) {
				level = candidate;
			}
		}
		if (level == null) {
			player.sendSystemMessage(Component.literal("Your home's dimension is gone."));
			return 0;
		}
		lastUse.put(name, now);
		player.teleportTo(level, home.x(), home.y(), home.z(), Set.of(), home.yaw(), home.pitch(), true);
		player.sendSystemMessage(Component.literal("Teleported home."));
		return 1;
	}

	void load() {
		try {
			if (!Files.exists(file)) {
				return;
			}
			for (String line : Files.readAllLines(file, StandardCharsets.UTF_8)) {
				String[] p = line.split(" ");
				if (p.length == 7) {
					homes.put(p[0], new Home(p[1], Double.parseDouble(p[2]), Double.parseDouble(p[3]),
							Double.parseDouble(p[4]), Float.parseFloat(p[5]), Float.parseFloat(p[6])));
				}
			}
		} catch (IOException | NumberFormatException e) {
			System.err.println("sethome: could not read " + file + ": " + e);
		}
	}

	void save() {
		StringBuilder out = new StringBuilder();
		for (Map.Entry<String, Home> e : homes.entrySet()) {
			Home h = e.getValue();
			out.append(e.getKey()).append(' ').append(h.dimension()).append(' ').append(h.x()).append(' ')
					.append(h.y()).append(' ').append(h.z()).append(' ').append(h.yaw()).append(' ')
					.append(h.pitch()).append('\n');
		}
		try {
			Files.createDirectories(file.getParent());
			Files.writeString(file, out.toString(), StandardCharsets.UTF_8);
		} catch (IOException e) {
			System.err.println("sethome: could not save " + file + ": " + e);
		}
	}
}
