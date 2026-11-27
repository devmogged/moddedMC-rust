package dev.moddedmc.companion;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class ScaffoldingConnectedTexturesTest {
    @Test
    void selectsTheInnerCornerAndExposedEdgeTiles() {
        assertEquals(54, ScaffoldingConnectedTextures.omni(true, true, true, true, true, true, true, true));
        assertEquals(53, ScaffoldingConnectedTextures.omni(true, true, true, true, true, false, true, true));
        assertEquals(41, ScaffoldingConnectedTextures.omni(true, true, false, true, false, true, false, false));
        assertEquals(13, ScaffoldingConnectedTextures.omni(false, true, true, false, false, false, true, false));
    }
}
