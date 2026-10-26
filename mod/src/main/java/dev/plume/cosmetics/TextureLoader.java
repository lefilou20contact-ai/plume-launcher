package dev.plume.cosmetics;

import com.mojang.blaze3d.platform.NativeImage;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.resources.ResourceLocation;

import java.io.ByteArrayInputStream;
import java.net.URI;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.time.Duration;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;

/** Télécharge une cape (PNG 64x32, 64 Ko maximum) et l'enregistre comme texture du jeu. */
final class TextureLoader {
    private TextureLoader() {}

    static ResourceLocation load(String url) {
        try {
            HttpRequest req = HttpRequest.newBuilder(URI.create(url)).timeout(Duration.ofSeconds(8)).header("User-Agent", "PlumeCosmetics").GET().build();
            HttpResponse<byte[]> res = Api.HTTP.send(req, HttpResponse.BodyHandlers.ofByteArray());
            byte[] bytes = res.body();
            if (res.statusCode() != 200 || bytes.length > 65536) return null;
            NativeImage image = NativeImage.read(new ByteArrayInputStream(bytes));
            if (image.getWidth() != 64 || image.getHeight() != 32) {
                image.close();
                return null;
            }
            ResourceLocation id = ResourceLocation.fromNamespaceAndPath(PlumeCosmetics.ID, "cape/" + sha1(url));
            Minecraft mc = Minecraft.getInstance();
            CompletableFuture<Boolean> done = new CompletableFuture<>();
            // L'enregistrement d'une texture doit se faire sur le fil de rendu.
            mc.execute(() -> {
                try {
                    mc.getTextureManager().register(id, new DynamicTexture(image));
                    done.complete(true);
                } catch (Throwable t) {
                    done.complete(false);
                }
            });
            return done.get(5, TimeUnit.SECONDS) ? id : null;
        } catch (Throwable t) {
            PlumeCosmetics.LOG.debug("Texture ignorée : {}", url, t);
            return null;
        }
    }

    private static String sha1(String s) throws Exception {
        StringBuilder sb = new StringBuilder();
        for (byte b : MessageDigest.getInstance("SHA-1").digest(s.getBytes(StandardCharsets.UTF_8))) sb.append(String.format("%02x", b));
        return sb.toString();
    }
}
