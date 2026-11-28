package dev.moddedmc.companion;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class ScreenLayoutTest {
    @Test
    void ivyEquipmentAndStorageRetainNativeSlots() throws Exception {
        var layout = ScreenLayout.parse(crucible(), "ivy_inventory");
        assertNotNull(layout);
        assertEquals(36, layout.containerSlots());
        assertEquals(32, layout.slots().size());
        assertEquals(new ScreenLayout.Position(4, 77, 61, null), layout.slots().get(4));
        assertEquals(new ScreenLayout.Position(9, 8, 83, null), layout.slots().get(5));
        assertEquals(new ScreenLayout.Position(35, 152, 119, null), layout.slots().getLast());
        assertEquals(210, layout.inventory().hotbarY());
    }

    private static String crucible() throws Exception {
        return Files.readString(Path.of("../mods/saints-dragons/resources/assets/saintsdragons/moddedmc/screens.json"));
    }

    @Test
    void releaseLayoutPreservesServerSlotIds() throws Exception {
        var layout = ScreenLayout.parse(crucible(), "draconic_crucible");
        assertNotNull(layout);
        assertEquals(176, layout.width());
        assertEquals(208, layout.height());
        assertEquals(27, layout.containerSlots());
        assertEquals(15, layout.stateSlot());
        assertEquals(11, layout.slots().size());
        assertEquals(new ScreenLayout.Position(6, 80, 21, null), layout.slots().getFirst());
        assertEquals(new ScreenLayout.Position(20, 98, 96, "processing"), layout.slots().getLast());
        assertEquals(15, layout.buttons().getFirst().slot());
        assertEquals(56, layout.layers().get(1).height());
        assertTrue(layout.labels().isEmpty());
    }

    @Test
    void malformedOrIncompatibleLayoutsFailClosed() throws Exception {
        String json = crucible();
        assertNull(ScreenLayout.parse(json, "unknown"));
        assertNull(ScreenLayout.parse("{broken", "draconic_crucible"));
        assertNull(ScreenLayout.parse(json.replace("\"version\": 1", "\"version\": 2"), "draconic_crucible"));
        assertNull(ScreenLayout.parse(json.replace("\"index\": 4", "\"index\": 6"), "draconic_crucible"));
        assertNull(ScreenLayout.parse(json.replace("\"x\": 80", "\"x\": 999999"), "draconic_crucible"));
        assertNull(ScreenLayout.parse(json.replace("textures/gui/", "textures/../"), "draconic_crucible"));
        assertNull(ScreenLayout.parse(json.replace("\"slot\": 15", "\"slot\": 6"), "draconic_crucible"));
    }
}
