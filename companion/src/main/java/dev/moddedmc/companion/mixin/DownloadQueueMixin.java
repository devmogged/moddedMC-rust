package dev.moddedmc.companion.mixin;

import com.google.common.hash.HashCode;
import com.google.common.hash.HashFunction;
import java.io.IOException;
import dev.moddedmc.companion.Companion;
import java.io.UncheckedIOException;
import java.net.Proxy;
import java.net.URL;
import java.nio.file.Path;
import java.util.Map;
import net.minecraft.server.packs.DownloadQueue;
import net.minecraft.util.HttpUtil;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Redirect;

@Mixin(DownloadQueue.class)
abstract class DownloadQueueMixin {
    @Redirect(method = "lambda$runDownload$0", at = @At(value = "INVOKE",
            target = "Lnet/minecraft/util/HttpUtil;downloadFile(Ljava/nio/file/Path;Ljava/net/URL;Ljava/util/Map;"
                    + "Lcom/google/common/hash/HashFunction;Lcom/google/common/hash/HashCode;ILjava/net/Proxy;"
                    + "Lnet/minecraft/util/HttpUtil$DownloadProgressListener;)Ljava/nio/file/Path;"))
    private Path moddedmc$download(Path directory, URL url, Map<String, String> headers, HashFunction hashFunction,
            HashCode hash, int maxSize, Proxy proxy, HttpUtil.DownloadProgressListener listener) {
        try {
            Path local = hash == null ? null : Companion.materialize(url, hash.toString(), proxy);
            if (local != null) return local;
        } catch (IOException error) {
            throw new UncheckedIOException("Could not assemble moddedMC assets", error);
        }
        return HttpUtil.downloadFile(directory, url, headers, hashFunction, hash, maxSize, proxy, listener);
    }
}
