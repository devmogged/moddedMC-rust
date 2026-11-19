package dev.moddedmc.companion;

import com.google.gson.Gson;
import com.google.gson.JsonParseException;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.List;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

public final class PackCache {
    private static final int MAX_MANIFEST = 2 * 1024 * 1024;
    private static final int MAX_ASSET = 8 * 1024 * 1024;
    private static final long MAX_PACK = 256L * 1024 * 1024;
    private static final Gson JSON = new Gson();

    private record Manifest(String pack_version, List<Asset> assets) {}
    private record Asset(String path, String sha256, Integer size) {}

    @FunctionalInterface
    public interface Fetch {
        InputStream open(String key) throws IOException;
    }

    public static Path materialize(Path cache, String id, Fetch fetch) throws IOException {
        if (!validHash(id)) throw new IOException("Invalid manifest hash");
        String key = "manifest-" + id + ".json";
        byte[] bytes = verified(cache.resolve(key), key, id, MAX_MANIFEST, -1, fetch);
        Manifest manifest = parse(bytes);
        Files.createDirectories(cache.resolve("assets"));
        Path zip = cache.resolve("pack-" + id + ".zip");
        Path temporary = Files.createTempFile(cache, "pack-", ".tmp");
        try {
            try (var out = new ZipOutputStream(Files.newOutputStream(temporary))) {
                for (Asset asset : manifest.assets()) {
                    String blob = "assets/" + asset.sha256();
                    byte[] data = verified(cache.resolve(blob), blob, asset.sha256(), asset.size(), asset.size(), fetch);
                    out.putNextEntry(new ZipEntry(asset.path()));
                    out.write(data);
                    out.closeEntry();
                }
            }
            Files.move(temporary, zip, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            return zip;
        } finally {
            Files.deleteIfExists(temporary);
        }
    }

    private static Manifest parse(byte[] bytes) throws IOException {
        Manifest manifest;
        try {
            manifest = JSON.fromJson(new String(bytes, StandardCharsets.UTF_8), Manifest.class);
        } catch (JsonParseException e) {
            throw new IOException("Invalid pack manifest", e);
        }
        if (manifest == null || !validHash(manifest.pack_version()) || manifest.assets() == null
                || manifest.assets().size() > 10_000) {
            throw new IOException("Invalid pack manifest");
        }
        var paths = new HashSet<String>();
        long total = 0;
        for (Asset asset : manifest.assets()) {
            if (asset == null || !validPath(asset.path()) || !paths.add(asset.path())
                    || !validHash(asset.sha256()) || asset.size() == null || asset.size() < 0 || asset.size() > MAX_ASSET) {
                throw new IOException("Invalid asset in pack manifest");
            }
            total += asset.size();
        }
        if (!paths.contains("pack.mcmeta") || total > MAX_PACK) throw new IOException("Invalid pack size or metadata");
        return manifest;
    }

    private static boolean validPath(String path) {
        if ("pack.mcmeta".equals(path)) return true;
        if (path == null || path.length() > 512 || !path.startsWith("assets/")) return false;
        for (String part : path.split("/", -1)) {
            if (part.equals(".") || part.equals("..") || !part.matches("[a-z0-9_.-]+")) return false;
        }
        return true;
    }

    private static boolean validHash(String hash) {
        return hash != null && hash.matches("[0-9a-f]{64}");
    }

    private static byte[] verified(Path path, String key, String hash, int max, int size, Fetch fetch) throws IOException {
        if (Files.isRegularFile(path) && Files.size(path) <= max) {
            try (var in = Files.newInputStream(path)) {
                byte[] data = in.readNBytes(max + 1);
                if (matches(data, hash, max, size)) return data;
            }
        }
        byte[] data;
        try (var in = fetch.open(key)) {
            data = in.readNBytes(max + 1);
        }
        if (!matches(data, hash, max, size)) throw new IOException("SHA-256 or size mismatch for " + key);
        Files.createDirectories(path.getParent());
        Path temporary = Files.createTempFile(path.getParent(), "asset-", ".tmp");
        try {
            Files.write(temporary, data);
            Files.move(temporary, path, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } finally {
            Files.deleteIfExists(temporary);
        }
        return data;
    }

    private static boolean matches(byte[] data, String hash, int max, int size) {
        if (data.length > max || (size >= 0 && data.length != size)) return false;
        try {
            return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(data)).equals(hash);
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException(e);
        }
    }
}
