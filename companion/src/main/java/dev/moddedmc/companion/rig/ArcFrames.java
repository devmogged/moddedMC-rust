package dev.moddedmc.companion.rig;

import com.mojang.blaze3d.vertex.VertexConsumer;
import com.mojang.blaze3d.vertex.PoseStack;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.client.renderer.rendertype.RenderTypes;
import net.minecraft.core.component.DataComponents;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.util.RandomSource;
import net.minecraft.world.entity.Display.ItemDisplay;
import net.minecraft.world.phys.Vec3;

/** Pack-defined, untextured lightning ribbons on an ordinary item display.
 *  The server supplies the segment and layer styles; rendering needs no GeckoLib. */
public final class ArcFrames {
    private static final RigFrame HIDDEN = (pose, collector, camera) -> {};
    private ArcFrames() {}

    record Window(float start, float end, float pulse, float alpha) {}
    record Layer(float scale, float width, float jitter, float r, float g, float b, float alpha) {}

    public static RigFrame extract(ItemDisplay display, float delta) {
        var state = display.itemRenderState();
        if (state == null) return null;
        var data = state.itemStack().get(DataComponents.CUSTOM_DATA);
        if (data == null) return null;
        var arc = data.copyTag().getCompound("moddedmc:arc").orElse(null);
        if (arc == null) return null;
        Vec3 segment = new Vec3(arc.getFloatOr("x", 0), arc.getFloatOr("y", 0), arc.getFloatOr("z", 0));
        double length = segment.length();
        if (!Double.isFinite(length) || length < 0.001 || length > 256) return HIDDEN;
        int lifetime = arc.getIntOr("lifetime", 0);
        if (lifetime < 1 || lifetime > 200) return HIDDEN;
        float scale = arc.getFloatOr("scale", 1);
        if (!Float.isFinite(scale) || scale < 0.1f || scale > 2) return HIDDEN;
        boolean grow = arc.getIntOr("grow", 0) != 0;
        var layers = readLayers(arc);
        if (layers.isEmpty()) return HIDDEN;
        int tick = display.tickCount;
        Window window = window(tick + delta, lifetime, grow);
        if (window.end <= window.start || window.alpha <= 0.01f) return HIDDEN;
        long seed = Integer.toUnsignedLong(arc.getIntOr("seed", 0)) + (grow ? 0 : tick * 31L);
        Vec3 start = segment.scale(-0.5), end = segment.scale(0.5);
        return (pose, collector, camera) -> collector.submitCustomGeometry(pose, RenderTypes.lightning(),
                (matrix, vertices) -> {
                    for (int i = 0; i < layers.size(); i++) {
                        renderLayer(matrix, vertices, start, end, layers.get(i),
                                scale * window.pulse, seed + (i == 2 ? 63 : i * 31L), window);
                    }
                });
    }

    private static List<Layer> readLayers(CompoundTag tag) {
        ListTag list = tag.getListOrEmpty("layers");
        if (list.isEmpty() || list.size() > 4) return List.of();
        var layers = new ArrayList<Layer>(list.size());
        for (int i = 0; i < list.size(); i++) {
            ListTag row = list.getListOrEmpty(i);
            if (row.size() != 7) return List.of();
            float[] values = new float[7];
            for (int j = 0; j < 7; j++) {
                values[j] = row.getFloatOr(j, Float.NaN);
                if (!Float.isFinite(values[j]) || values[j] < 0 || values[j] > 1) return List.of();
            }
            layers.add(new Layer(values[0], values[1], values[2], values[3], values[4], values[5], values[6]));
        }
        return List.copyOf(layers);
    }

    private static float clamp(float value) { return Math.clamp(value, 0, 1); }

    static Window window(float age, int lifetime, boolean grow) {
        if (!grow) {
            float fade = Math.min(1.5f, Math.max(0.75f, lifetime * 0.4f));
            return new Window(0, 1, 1, clamp((lifetime - age) / fade));
        }
        float growTicks = Math.max(2, lifetime * 0.34f);
        float retractStart = growTicks + 0.75f;
        float retractTicks = Math.max(1.5f, Math.min(2.75f, lifetime - retractStart));
        float inverse = 1 - clamp(age / growTicks);
        float t = clamp((age - retractStart) / retractTicks);
        return new Window(t * t * (3 - 2 * t), 1 - inverse * inverse * inverse,
                age >= growTicks && age < retractStart ? 1.32f : 1, 1);
    }

    private static void renderLayer(PoseStack.Pose pose, VertexConsumer consumer, Vec3 start, Vec3 end,
                                    Layer layer, float scale, long seed, Window window) {
        Vec3 direction = end.subtract(start).normalize();
        Vec3 axisA = direction.cross(Math.abs(direction.y) > 0.85 ? new Vec3(1, 0, 0) : new Vec3(0, 1, 0)).normalize();
        Vec3 axisB = direction.cross(axisA).normalize();
        Vec3 diagA = axisA.add(axisB).normalize(), diagB = axisA.subtract(axisB).normalize();
        scale *= layer.scale;
        float width = (0.02f + scale * layer.width) * 1.4f;
        float jitter = 0.006f + scale * layer.jitter;
        float alpha = window.alpha * layer.alpha;
        Vec3[] points = new Vec3[8];
        var random = RandomSource.create(seed);
        float a = 0, b = 0;
        for (int i = 7; i >= 0; i--) {
            points[i] = start.lerp(end, i / 7f).add(axisA.scale(a * jitter)).add(axisB.scale(b * jitter));
            a += random.nextInt(11) - 5;
            b += random.nextInt(11) - 5;
        }
        for (int i = 0; i < 7; i++) {
            float t0 = i / 7f, t1 = (i + 1) / 7f;
            float lo = Math.max(t0, window.start), hi = Math.min(t1, window.end);
            if (hi <= lo) continue;
            Vec3 p0 = points[i].lerp(points[i + 1], (lo - t0) / (t1 - t0));
            Vec3 p1 = points[i].lerp(points[i + 1], (hi - t0) / (t1 - t0));
            float w0 = width * (1.08f - lo * 0.22f), w1 = width * (1.08f - hi * 0.22f);
            ribbon(pose, consumer, p0, p1, axisA, w0, w1, layer, alpha);
            ribbon(pose, consumer, p0, p1, axisB, w0, w1, layer, alpha);
            ribbon(pose, consumer, p0, p1, diagA, w0 * 0.82f, w1 * 0.82f, layer, alpha * 0.92f);
            ribbon(pose, consumer, p0, p1, diagB, w0 * 0.82f, w1 * 0.82f, layer, alpha * 0.92f);
        }
    }

    private static void ribbon(PoseStack.Pose pose, VertexConsumer out, Vec3 start, Vec3 end, Vec3 axis,
                               float w0, float w1, Layer layer, float alpha) {
        Vec3 a = axis.scale(w0), b = axis.scale(w1);
        vertex(pose, out, start.add(a), layer, alpha);
        vertex(pose, out, end.add(b), layer, alpha);
        vertex(pose, out, end.subtract(b), layer, alpha);
        vertex(pose, out, start.subtract(a), layer, alpha);
    }

    private static void vertex(PoseStack.Pose pose, VertexConsumer out, Vec3 pos, Layer layer, float alpha) {
        out.addVertex(pose, (float) pos.x, (float) pos.y, (float) pos.z).setColor(layer.r, layer.g, layer.b, alpha);
    }
}
