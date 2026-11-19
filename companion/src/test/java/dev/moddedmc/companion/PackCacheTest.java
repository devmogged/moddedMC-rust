package dev.moddedmc.companion;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import java.util.zip.ZipFile;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

import static org.junit.jupiter.api.Assertions.*;

class PackCacheTest {
    @TempDir Path cache;
    private static final byte[] META = "{}".getBytes(StandardCharsets.UTF_8);
    private static final byte[] MODEL = "model".getBytes(StandardCharsets.UTF_8);

    @Test
    void fetchesOnlyMissingBlobsAndRebuildsOffline() throws Exception {
        var requests = new ArrayList<String>();
        byte[] manifest = manifest("assets/example/model.json");
        var id = hash(manifest);
        Files.createDirectories(cache.resolve("assets"));
        Files.write(cache.resolve("assets").resolve(hash(META)), META);
        Path zip = PackCache.materialize(cache, id, key -> {
            requests.add(key);
            return new ByteArrayInputStream(key.startsWith("manifest-") ? manifest : MODEL);
        });
        assertEquals(List.of("manifest-" + id + ".json", "assets/" + hash(MODEL)), requests);
        Files.delete(zip);
        zip = PackCache.materialize(cache, id, key -> { throw new AssertionError(key); });
        try (var pack = new ZipFile(zip.toFile())) {
            var entry = pack.getEntry("assets/example/model.json");
            assertArrayEquals(MODEL, pack.getInputStream(entry).readAllBytes());
        }
        Files.write(cache.resolve("assets").resolve(hash(MODEL)), new byte[MODEL.length]);
        assertThrows(IOException.class, () -> PackCache.materialize(cache, id,
                key -> new ByteArrayInputStream(new byte[MODEL.length])));
    }

    @Test
    void rejectsUnverifiedManifestsAndUnsafePaths() throws Exception {
        var manifest = manifest("assets/example/../escape");
        assertThrows(IOException.class, () -> PackCache.materialize(cache, "0".repeat(64),
                key -> new ByteArrayInputStream(manifest)));
        var requests = new ArrayList<String>();
        assertThrows(IOException.class, () -> PackCache.materialize(cache, hash(manifest), key -> {
            requests.add(key);
            return new ByteArrayInputStream(manifest);
        }));
        assertEquals(1, requests.size());
    }

    private static byte[] manifest(String path) throws Exception {
        return """
                {"pack_version":"%s","assets":[
                  {"path":"pack.mcmeta","sha256":"%s","size":2},
                  {"path":"%s","sha256":"%s","size":5}]}
                """.formatted("0".repeat(64), hash(META), path, hash(MODEL)).getBytes(StandardCharsets.UTF_8);
    }

    private static String hash(byte[] data) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(data));
    }
}
