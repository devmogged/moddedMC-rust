package dev.moddedmc.companion;

import com.mojang.math.Transformation;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.minecraft.client.Minecraft;
import net.minecraft.core.BlockPos;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;
import net.minecraft.world.entity.Display;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.CollisionContext;
import org.joml.Vector3f;

public final class BlockDisplayUse {
    private BlockDisplayUse() {}

    static void register() {
        PayloadTypeRegistry.serverboundPlay().register(Packet.TYPE, Packet.CODEC);
    }

    public static boolean use(Minecraft client) {
        var player = client.player;
        var connection = client.getConnection();
        if (!Companion.canUseBlockDisplays() || player == null || player.isSpectator()
                || !player.isAlive() || client.level == null || connection == null) return false;

        // Use the same interpolation as the display renderer and stop at the vanilla crosshair hit.
        float partialTick = client.getDeltaTracker().getGameTimeDeltaPartialTick(true);
        Vec3 from = player.getEyePosition(partialTick);
        double reach = player.blockInteractionRange();
        if (client.hitResult != null) reach = Math.min(reach, from.distanceTo(client.hitResult.getLocation()));
        Vec3 to = from.add(player.getViewVector(partialTick).scale(reach));
        var context = CollisionContext.of(player);
        double nearest = reach * reach;
        Display.BlockDisplay selected = null;
        for (var entity : client.level.entitiesForRendering()) {
            if (!(entity instanceof Display.BlockDisplay display)) continue;
            var state = display.blockRenderState();
            var render = display.renderState();
            if (state == null || render == null || render.billboardConstraints() != Display.BillboardConstraints.FIXED) {
                continue;
            }
            var origin = display.getPosition(partialTick);
            var transform = render.transformation().get(display.calculateInterpolationProgress(partialTick));
            var inverse = transform.inverse();
            if (inverse == null) continue;

            var localFrom = transformPoint(inverse, from.subtract(origin));
            var localTo = transformPoint(inverse, to.subtract(origin));
            var shape = state.blockState().getShape(client.level, BlockPos.ZERO, context);
            var hit = shape.clip(localFrom, localTo, BlockPos.ZERO);
            if (hit == null) continue;
            double distance = from.distanceToSqr(origin.add(transformPoint(transform, hit.getLocation())));
            if (distance < nearest) {
                nearest = distance;
                selected = display;
            }
        }
        if (selected == null || !Companion.isServerTrapdoor(selected.blockRenderState().blockState())) return false;
        connection.send(new ServerboundCustomPayloadPacket(new Packet(selected.getId())));
        return true;
    }

    private static Vec3 transformPoint(Transformation transform, Vec3 pos) {
        var point = transform.getMatrix().transformPosition(new Vector3f((float)pos.x, (float)pos.y, (float)pos.z));
        return new Vec3(point.x, point.y, point.z);
    }

    private record Packet(int display) implements CustomPacketPayload {
        private static final Type<Packet> TYPE = new Type<>(Identifier.fromNamespaceAndPath("moddedmc", "block_display_use"));
        private static final StreamCodec<FriendlyByteBuf, Packet> CODEC = StreamCodec.of(
                (out, packet) -> out.writeInt(packet.display()), in -> new Packet(in.readInt()));

        @Override
        public Type<Packet> type() {
            return TYPE;
        }
    }
}
