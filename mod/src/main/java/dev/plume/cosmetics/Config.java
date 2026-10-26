package dev.plume.cosmetics;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import net.fabricmc.loader.api.FabricLoader;

import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/** Réglages : valeurs par défaut du fichier intégré, puis config/plume-cosmetics.json s'il existe. */
public final class Config {
    private static boolean enabled = true;
    private static String backend = "";

    private Config() {}

    public static void load() {
        try (var in = Config.class.getResourceAsStream("/plume_cosmetics_defaults.json")) {
            if (in != null) apply(JsonParser.parseReader(new InputStreamReader(in, StandardCharsets.UTF_8)).getAsJsonObject());
        } catch (Exception e) {
            PlumeCosmetics.LOG.warn("Réglages par défaut illisibles", e);
        }
        Path file = FabricLoader.getInstance().getConfigDir().resolve("plume-cosmetics.json");
        try {
            if (Files.exists(file)) apply(JsonParser.parseString(Files.readString(file)).getAsJsonObject());
            else Files.writeString(file, "{\n  \"enabled\": true,\n  \"backendUrl\": \"\"\n}\n");
        } catch (Exception e) {
            PlumeCosmetics.LOG.warn("Réglages illisibles : {}", file, e);
        }
    }

    private static void apply(JsonObject o) {
        if (o.has("enabled") && o.get("enabled").isJsonPrimitive()) enabled = o.get("enabled").getAsBoolean();
        if (o.has("backendUrl") && o.get("backendUrl").isJsonPrimitive()) {
            String u = o.get("backendUrl").getAsString().trim();
            // Une adresse vide dans le fichier de l'utilisateur garde l'adresse par défaut.
            if (!u.isEmpty()) backend = u.endsWith("/") ? u.substring(0, u.length() - 1) : u;
        }
    }

    public static boolean enabled() { return enabled; }

    /** Adresse du serveur (https obligatoire), sans « / » final ; vide si non configurée. */
    public static String backend() { return backend.startsWith("https://") ? backend : ""; }
}
