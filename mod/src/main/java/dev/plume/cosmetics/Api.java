package dev.plume.cosmetics;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.stream.Collectors;

/** Appels au serveur de cosmétiques. Seuls les identifiants (UUID) des joueurs visibles sont envoyés. */
final class Api {
    static final HttpClient HTTP = HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(5))
            .followRedirects(HttpClient.Redirect.NORMAL)
            .build();

    private Api() {}

    /** Renvoie, pour chaque joueur qui porte une cape, l'adresse de sa texture. */
    static Map<UUID, String> lookup(List<UUID> ids) throws Exception {
        String base = Config.backend();
        Map<UUID, String> out = new HashMap<>();
        if (base.isEmpty() || ids.isEmpty()) return out;
        String q = ids.stream().map(u -> u.toString().replace("-", "")).collect(Collectors.joining(","));
        HttpRequest req = HttpRequest.newBuilder(URI.create(base + "/plume/lookup?uuids=" + q))
                .timeout(Duration.ofSeconds(6))
                .header("User-Agent", "PlumeCosmetics")
                .GET().build();
        HttpResponse<String> res = HTTP.send(req, HttpResponse.BodyHandlers.ofString());
        if (res.statusCode() != 200) return out;
        JsonObject root = JsonParser.parseString(res.body()).getAsJsonObject();
        if (!root.has("players") || !root.get("players").isJsonObject()) return out;
        for (var e : root.getAsJsonObject("players").entrySet()) {
            try {
                if (!e.getValue().isJsonObject()) continue;
                JsonObject p = e.getValue().getAsJsonObject();
                if (!p.has("cape") || p.get("cape").isJsonNull()) continue;
                String url = p.get("cape").getAsString();
                if (allowed(url)) out.put(parse(e.getKey()), url);
            } catch (Exception ignored) {
                // une entrée invalide n'empêche pas les autres
            }
        }
        return out;
    }

    /** Les textures ne peuvent venir que du même serveur que celui configuré. */
    static boolean allowed(String url) {
        try {
            URI u = URI.create(url), b = URI.create(Config.backend());
            return "https".equals(u.getScheme()) && u.getHost() != null && u.getHost().equals(b.getHost());
        } catch (Exception e) {
            return false;
        }
    }

    static UUID parse(String hex) {
        String h = hex.replace("-", "");
        return new UUID(Long.parseUnsignedLong(h.substring(0, 16), 16), Long.parseUnsignedLong(h.substring(16), 16));
    }
}
