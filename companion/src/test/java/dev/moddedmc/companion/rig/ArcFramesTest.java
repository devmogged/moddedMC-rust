package dev.moddedmc.companion.rig;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class ArcFramesTest {
    @Test
    void growingArcFlashesThenRetracts() {
        var initial = ArcFrames.window(0, 5, true);
        assertEquals(initial.start(), initial.end());
        var growing = ArcFrames.window(1, 5, true);
        assertEquals(0, growing.start(), 1e-6);
        assertEquals(0.875f, growing.end(), 1e-6);
        var flash = ArcFrames.window(2.5f, 5, true);
        assertEquals(1, flash.end(), 1e-6);
        assertEquals(1.32f, flash.pulse(), 1e-6);
        var retract = ArcFrames.window(4, 5, true);
        assertTrue(retract.start() > 0 && retract.start() < 1);
        var expired = ArcFrames.window(5, 5, true);
        assertEquals(expired.start(), expired.end());
    }

    @Test
    void fadingArcRetainsFullLengthAndFadesAtEnd() {
        var full = ArcFrames.window(3.5f, 5, false);
        assertEquals(0, full.start(), 1e-6);
        assertEquals(1, full.end(), 1e-6);
        assertEquals(1, full.alpha(), 1e-6);
        assertEquals(0.5f, ArcFrames.window(4.25f, 5, false).alpha(), 1e-6);
        assertEquals(0, ArcFrames.window(5, 5, false).alpha(), 1e-6);
    }
}
