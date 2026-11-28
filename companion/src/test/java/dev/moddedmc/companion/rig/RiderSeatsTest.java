package dev.moddedmc.companion.rig;

import net.minecraft.world.phys.Vec3;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class RiderSeatsTest {
    @Test
    void sampledOffsetFollowsNewMountYawWithoutChangingHeight() {
        Vec3 local = RiderSeats.toLocal(new Vec3(2, 3, 0), 90);
        assertEquals(0, local.x, 1e-6);
        assertEquals(3, local.y, 1e-6);
        assertEquals(-2, local.z, 1e-6);
        Vec3 turned = RiderSeats.toWorld(local, 180);
        assertEquals(0, turned.x, 1e-6);
        assertEquals(3, turned.y, 1e-6);
        assertEquals(2, turned.z, 1e-6);
    }

    @Test
    void groundLocatorsExpireAfterTwoTicks() {
        assertTrue(RiderSeats.fresh(10, 10, 2));
        assertTrue(RiderSeats.fresh(10, 12, 2));
        assertFalse(RiderSeats.fresh(10, 13, 2));
        assertFalse(RiderSeats.fresh(10, 9, 2));
        assertTrue(RiderSeats.fresh(10, 100, Integer.MAX_VALUE));
    }
}
