package dev.moddedmc.companion.rig;

import java.lang.ref.WeakReference;
import java.util.Map;
import java.util.WeakHashMap;
import net.minecraft.core.component.DataComponents;
import net.minecraft.util.Mth;
import net.minecraft.world.entity.Display.ItemDisplay;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;

/** Client-only seat positions; no dependency on GeckoLib for the passenger hooks. */
public final class RiderSeats {
    private static final Map<Entity, Sample> SAMPLES = new WeakHashMap<>();

    private RiderSeats() {}

    record Anchor(Vec3 local, int tick, int maxAge) {}

    private record Sample(WeakReference<Entity> mount, WeakReference<ItemDisplay> display, Anchor[] anchors) {}

    static synchronized void clear() {
        SAMPLES.clear();
    }

    static Anchor anchor(Entity mount, float delta, Vec3 world, int maxAge) {
        return new Anchor(toLocal(world.subtract(mount.getPosition(delta)),
                Mth.rotLerp(delta, mount.yRotO, mount.getYRot())), mount.tickCount, maxAge);
    }

    static synchronized void sample(ItemDisplay display, Entity mount, Anchor[] anchors) {
        SAMPLES.put(mount, new Sample(new WeakReference<>(mount), new WeakReference<>(display), anchors));
    }

    /** Reapply the sampled offset to the mount's current position and yaw, as Java does. */
    public static synchronized Vec3 position(Entity mount, Entity rider, float delta) {
        if (mount == null || rider.getVehicle() != mount) return null;
        var sample = SAMPLES.get(mount);
        if (sample == null || sample.mount().get() != mount) return null;
        var display = sample.display().get();
        if (display == null || display.isRemoved() || display.level() != mount.level()) return null;
        var item = display.itemRenderState();
        if (item == null) return null;
        var data = item.itemStack().get(DataComponents.CUSTOM_DATA);
        if (data == null || data.copyTag().getIntOr("moddedmc:rig_mount", -1) != mount.getId()) return null;
        int index = mount.getPassengers().indexOf(rider);
        if (index < 0 || index >= sample.anchors().length) return null;
        var anchor = sample.anchors()[index];
        if (anchor == null || !fresh(anchor.tick(), mount.tickCount, anchor.maxAge())) return null;
        return mount.getPosition(delta).add(toWorld(anchor.local(),
                Mth.rotLerp(delta, mount.yRotO, mount.getYRot())));
    }

    static boolean fresh(int sampled, int now, int maxAge) {
        return now >= sampled && (long) now - sampled <= maxAge;
    }

    static Vec3 toLocal(Vec3 offset, float yaw) {
        return toWorld(offset, -yaw);
    }

    static Vec3 toWorld(Vec3 offset, float yaw) {
        double angle = Math.toRadians(yaw);
        double sin = Math.sin(angle), cos = Math.cos(angle);
        return new Vec3(offset.x * cos - offset.z * sin, offset.y, offset.x * sin + offset.z * cos);
    }
}
