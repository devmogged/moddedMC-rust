package dev.moddedmc.companion;

import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class StandaloneLayoutTest {
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
        assertTrue(layout.controls().stream().anyMatch(c -> c.input() && c.action().equals("add_name")));
        assertNull(StandaloneLayout.parse(json, "draconic_crucible"));
        assertNull(StandaloneLayout.parse(json.replace("textures/gui/", "textures/../"), "draconic_codex"));
        assertNull(StandaloneLayout.parse(json.replace("\"maxLength\": 16", "\"maxLength\": 1000"), "draconic_codex"));
    }
}
