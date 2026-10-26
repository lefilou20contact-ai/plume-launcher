package dev.plume.cosmetics;

import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientPacketListener;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.resources.ResourceLocation;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;

/** Garde en mémoire la cape de chaque joueur visible et la rafraîchit toutes les 20 secondes. */
public final class CosmeticsCache {
    private static final Map<UUID, ResourceLocation> CAPES = new ConcurrentHashMap<>();
    private static final Map<String, ResourceLocation> TEXTURES = new ConcurrentHashMap<>();
    private static final Set<String> FAILED = ConcurrentHashMap.newKeySet();

    private CosmeticsCache() {}

    /** Appelé par le jeu à chaque image : simple lecture, aucun appel réseau. */
    public static ResourceLocation capeFor(UUID id) { return id == null ? null : CAPES.get(id); }

    static void start() {
        ScheduledExecutorService pool = Executors.newSingleThreadScheduledExecutor(r -> {
            Thread t = new Thread(r, "Plume Cosmetics");
            t.setDaemon(true);
            return t;
        });
        pool.scheduleWithFixedDelay(CosmeticsCache::tick, 3, 20, TimeUnit.SECONDS);
    }

    private static void tick() {
        try {
            Minecraft mc = Minecraft.getInstance();
            CompletableFuture<Set<UUID>> future = new CompletableFuture<>();
            mc.execute(() -> {
                Set<UUID> seen = new HashSet<>();
                try {
                    ClientPacketListener conn = mc.getConnection();
                    if (conn != null) for (PlayerInfo p : conn.getOnlinePlayers()) seen.add(p.getProfile().getId());
                } catch (Throwable ignored) {
                    // liste indisponible : on réessaie au prochain passage
                }
                future.complete(seen);
            });
            Set<UUID> visible = future.get(5, TimeUnit.SECONDS);
            CAPES.keySet().removeIf(id -> !visible.contains(id));
            if (visible.isEmpty()) return;
            List<UUID> list = new ArrayList<>(visible);
            Map<UUID, String> urls = new HashMap<>();
            for (int i = 0; i < list.size(); i += 40) urls.putAll(Api.lookup(list.subList(i, Math.min(list.size(), i + 40))));
            for (UUID id : list) {
                String url = urls.get(id);
                if (url == null) { CAPES.remove(id); continue; }
                ResourceLocation tex = TEXTURES.get(url);
                if (tex == null && !FAILED.contains(url)) {
                    tex = TextureLoader.load(url);
                    if (tex != null) TEXTURES.put(url, tex); else FAILED.add(url);
                }
                if (tex != null) CAPES.put(id, tex); else CAPES.remove(id);
            }
        } catch (Throwable t) {
            PlumeCosmetics.LOG.debug("Mise à jour des cosmétiques impossible", t);
        }
    }
}
