package dev.moddedmc.companion;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class StandaloneLayoutTest {
    @Test
    void dialogueUsesBoundedAtlasAndTypedRows() throws Exception {
        String json = Files.readString(Path.of("../mods/saints-dragons/resources/assets/saintsdragons/moddedmc/screens.json"));
        var layout = StandaloneLayout.parse(json, "ivy_dialogue");
        assertNotNull(layout);
        assertTrue(layout.tabs().isEmpty());
        assertTrue(layout.bottomAligned());
        assertEquals(80, layout.backgroundV());
        assertEquals(96, layout.backgroundHeight());
        assertEquals("name_input", layout.input().visibleWhen());
        assertEquals("typed_action", layout.lists().getFirst().inputActionField());
        assertEquals(120, layout.dialogue().delayMs());
        assertEquals(36, layout.dialogue().intervalMs());
        assertEquals(72, layout.dialogue().voiceIntervalMs());
        assertEquals("speaker_id", layout.dialogue().speakerField());
        assertNull(StandaloneLayout.parse(json.replace("\"intervalMs\": 36", "\"intervalMs\": 0"), "ivy_dialogue"));
        assertNull(StandaloneLayout.parse(json.replace("\"backgroundV\": 80", "\"backgroundV\": 200"), "ivy_dialogue"));
    }

    @Test
    void codexUsesReleasedGeometryAndServerActions() throws Exception {
        String json = Files.readString(Path.of("../mods/saints-dragons/resources/assets/saintsdragons/moddedmc/screens.json"));
        var layout = StandaloneLayout.parse(json, "draconic_codex");
        assertNotNull(layout);
        assertEquals(393, layout.width());
        assertEquals(214, layout.height());
        assertEquals(3, layout.tabs().size());
        assertEquals(16, layout.input().maxLength());
        assertEquals(17, layout.pages().lines());
        assertEquals("items", layout.pages().itemField());
        var preview = layout.previews().getFirst();
        assertEquals(113, preview.x());
        assertEquals(51, preview.y());
        assertEquals(85, preview.width());
        assertEquals(85, preview.height());
        var animated = layout.layers().stream().filter(l -> l.frames() != null).findFirst().orElseThrow();
        assertEquals(3, animated.frameTicks());
        assertEquals(2, animated.frames().size());
        assertNull(StandaloneLayout.parse(json.replace("\"frameTicks\": 3", "\"frameTicks\": 0"), "draconic_codex"));
        assertNull(StandaloneLayout.parse(json.replace("\"width\": 85", "\"width\": 500"), "draconic_codex"));
        assertTrue(layout.controls().stream().anyMatch(c -> c.input() && c.action().equals("add_name")));
        assertNull(StandaloneLayout.parse(json, "draconic_crucible"));
        assertNull(StandaloneLayout.parse(json.replace("textures/gui/", "textures/../"), "draconic_codex"));
        assertNull(StandaloneLayout.parse(json.replace("\"maxLength\": 16", "\"maxLength\": 1000"), "draconic_codex"));
    }
}
