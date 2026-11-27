package dev.moddedmc.companion;

import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class MountedInputTest {
    @Test void mouseEdgesAndHeldCadence() {
        var input = new MountedInput();
        var phases = new ArrayList<Integer>();
        for (int i = 0; i < 8; i++) input.mouse(0, true, i == 0, phases::add);
        input.mouse(0, false, false, phases::add);
        input.mouse(1, false, true, phases::add);
        assertEquals(List.of(1, 2, 2, 4, 1, 4), phases);
    }

    @Test void directionEdgesHaveIndependentStrictWindows() {
        var input = new MountedInput();
        assertFalse(input.tap(0, true, 0));
        assertFalse(input.tap(0, true, 10));
        assertFalse(input.tap(0, false, 20));
        assertFalse(input.tap(1, true, 30));
        assertTrue(input.tap(0, true, 299_999_999));
        assertFalse(input.tap(1, false, 300_000_000));
        assertFalse(input.tap(1, true, 300_000_030));
        input.reset();
        assertFalse(input.tap(0, true, 300_000_031));
    }
}
