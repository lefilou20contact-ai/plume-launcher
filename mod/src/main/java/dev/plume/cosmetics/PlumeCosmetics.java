package dev.plume.cosmetics;

import net.fabricmc.api.ClientModInitializer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/** Point d'entrée du mod : charge la configuration puis démarre la mise à jour des cosmétiques. */
public class PlumeCosmetics implements ClientModInitializer {
    public static final String ID = "plume_cosmetics";
    public static final Logger LOG = LoggerFactory.getLogger("Plume Cosmetics");

    @Override
    public void onInitializeClient() {
        Config.load();
        if (!Config.enabled() || Config.backend().isEmpty()) {
            LOG.info("Plume Cosmetics est inactif (désactivé ou serveur non configuré).");
            return;
        }
        CosmeticsCache.start();
        LOG.info("Plume Cosmetics actif.");
    }
}
