// Plume : serveur des cosmétiques (Supabase Edge Function, une seule fonction, plusieurs routes).
//   GET  /plume/catalog            catalogue public
//   POST /plume/auth/challenge     identifiant de défi à signer auprès de Mojang
//   POST /plume/auth/verify        {name, server_id} -> jeton (après vérification hasJoined chez Mojang)
//   GET  /plume/me                 cosmétiques possédés et équipés (jeton requis)
//   POST /plume/equip              {kind, id|null} (jeton requis)
//   GET  /plume/lookup?uuids=a,b   capes des joueurs demandés (public, utilisé par le mod)
import { createClient } from "jsr:@supabase/supabase-js@2";

const SUPABASE_URL = Deno.env.get("SUPABASE_URL")!;
const db = createClient(SUPABASE_URL, Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!, { auth: { persistSession: false } });
const SECRET = Deno.env.get("PLUME_JWT_SECRET") ?? "";
const enc = new TextEncoder();
const KINDS = ["cape", "hat", "wings"];
const publicUrl = (path: string | null) => path ? `${SUPABASE_URL}/storage/v1/object/public/cosmetics/${path}` : null;

const reply = (body: unknown, status = 200, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", "access-control-allow-origin": "*", ...headers } });

// ---------- jeton signé (HMAC-SHA256), valable 1 heure ----------
const b64 = (u: Uint8Array) => btoa(String.fromCharCode(...u)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
async function sign(data: string): Promise<string> {
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return b64(new Uint8Array(await crypto.subtle.sign("HMAC", key, enc.encode(data))));
}
async function makeToken(p: { uuid: string; name: string }) {
  const body = b64(enc.encode(JSON.stringify({ ...p, exp: Math.floor(Date.now() / 1000) + 3600 })));
  return `${body}.${await sign(body)}`;
}
async function readToken(header: string | null): Promise<{ uuid: string; name: string } | null> {
  const [body, sig] = (header ?? "").replace(/^Bearer /, "").split(".");
  if (!body || !sig || !SECRET) return null;
  const good = await sign(body);
  if (good.length !== sig.length) return null;
  let diff = 0;
  for (let i = 0; i < good.length; i++) diff |= good.charCodeAt(i) ^ sig.charCodeAt(i);
  if (diff) return null;
  try {
    const p = JSON.parse(atob(body.replace(/-/g, "+").replace(/_/g, "/")));
    return p.exp > Date.now() / 1000 ? p : null;
  } catch { return null; }
}

const isHex32 = (s: unknown) => typeof s === "string" && /^[0-9a-f]{32}$/.test(s);
const dash = (h: string) => `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
const undash = (u: string) => u.replace(/-/g, "");

Deno.serve(async (req: Request) => {
  if (req.method === "OPTIONS") return new Response(null, { status: 204, headers: { "access-control-allow-origin": "*", "access-control-allow-headers": "authorization, content-type", "access-control-allow-methods": "GET, POST, OPTIONS" } });
  const parts = new URL(req.url).pathname.split("/");
  const route = "/" + parts.slice(parts.indexOf("plume") + 1).join("/");
  try {
    // ---- catalogue ----
    if (req.method === "GET" && route === "/catalog") {
      const { data } = await db.from("cosmetics").select("id,kind,name,description,price_cents,preview_path").eq("published", true).order("created_at");
      return reply({ items: (data ?? []).map((c: any) => ({ id: c.id, kind: c.kind, name: c.name, description: c.description, price_cents: c.price_cents, preview_url: publicUrl(c.preview_path) })) }, 200, { "cache-control": "public, max-age=60" });
    }

    // ---- identité : défi puis vérification auprès de Mojang ----
    if (req.method === "POST" && route === "/auth/challenge") {
      await db.from("auth_challenges").delete().lt("created_at", new Date(Date.now() - 600_000).toISOString());
      const id = Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
      await db.from("auth_challenges").insert({ server_id: id });
      return reply({ server_id: id });
    }
    if (req.method === "POST" && route === "/auth/verify") {
      const { name, server_id } = await req.json();
      if (typeof name !== "string" || !/^[A-Za-z0-9_]{1,16}$/.test(name) || !isHex32(server_id)) return reply({ error: "requête invalide" }, 400);
      const { data: ch } = await db.from("auth_challenges").select("created_at").eq("server_id", server_id).maybeSingle();
      await db.from("auth_challenges").delete().eq("server_id", server_id); // usage unique
      if (!ch || Date.now() - new Date(ch.created_at).getTime() > 120_000) return reply({ error: "défi expiré" }, 401);
      const r = await fetch(`https://sessionserver.mojang.com/session/minecraft/hasJoined?username=${encodeURIComponent(name)}&serverId=${server_id}`);
      if (r.status !== 200) return reply({ error: "identité non confirmée par Mojang" }, 401);
      const prof = await r.json();
      if (!isHex32(prof.id)) return reply({ error: "profil invalide" }, 401);
      await db.from("players").upsert({ uuid: dash(prof.id), name: prof.name, last_seen: new Date().toISOString() });
      // Les cosmétiques gratuits appartiennent à tout le monde.
      const { data: free } = await db.from("cosmetics").select("id").eq("published", true).eq("price_cents", 0);
      if (free?.length) await db.from("ownership").upsert(free.map((c: any) => ({ player: dash(prof.id), cosmetic: c.id, source: "grant" })), { onConflict: "player,cosmetic", ignoreDuplicates: true });
      return reply({ token: await makeToken({ uuid: prof.id, name: prof.name }), uuid: prof.id, expires_in: 3600 });
    }

    // ---- compte : possédés et équipés ----
    if (req.method === "GET" && route === "/me") {
      const who = await readToken(req.headers.get("authorization"));
      if (!who) return reply({ error: "non autorisé" }, 401);
      const [own, eq] = await Promise.all([
        db.from("ownership").select("cosmetic").eq("player", dash(who.uuid)),
        db.from("equipped").select("cape,hat,wings").eq("player", dash(who.uuid)).maybeSingle(),
      ]);
      return reply({ owned: (own.data ?? []).map((o: any) => o.cosmetic), equipped: eq.data ?? {} });
    }
    if (req.method === "POST" && route === "/equip") {
      const who = await readToken(req.headers.get("authorization"));
      if (!who) return reply({ error: "non autorisé" }, 401);
      const { kind, id } = await req.json();
      if (!KINDS.includes(kind)) return reply({ error: "type invalide" }, 400);
      if (id !== null) {
        if (typeof id !== "string" || id.length > 64) return reply({ error: "identifiant invalide" }, 400);
        const [own, item] = await Promise.all([
          db.from("ownership").select("cosmetic").eq("player", dash(who.uuid)).eq("cosmetic", id).maybeSingle(),
          db.from("cosmetics").select("kind").eq("id", id).eq("published", true).maybeSingle(),
        ]);
        if (!own.data || !item.data || item.data.kind !== kind) return reply({ error: "cosmétique indisponible" }, 403);
      }
      await db.from("equipped").upsert({ player: dash(who.uuid), [kind]: id, updated_at: new Date().toISOString() });
      return reply({ ok: true });
    }

    // ---- lecture publique pour le mod ----
    if (req.method === "GET" && route === "/lookup") {
      const ids = (new URL(req.url).searchParams.get("uuids") ?? "").split(",").filter(isHex32).slice(0, 50);
      if (!ids.length) return reply({ players: {} });
      const { data: rows } = await db.from("equipped").select("player,cape").in("player", ids.map(dash));
      const capeIds = [...new Set((rows ?? []).map((r: any) => r.cape).filter(Boolean))];
      const paths: Record<string, string> = {};
      if (capeIds.length) {
        const { data: cs } = await db.from("cosmetics").select("id,asset_path").in("id", capeIds).eq("published", true);
        for (const c of cs ?? []) paths[c.id] = c.asset_path;
      }
      const players: Record<string, { cape: string | null }> = {};
      for (const r of rows ?? []) players[undash(r.player)] = { cape: r.cape && paths[r.cape] ? publicUrl(paths[r.cape]) : null };
      return reply({ players }, 200, { "cache-control": "public, max-age=15" });
    }
    return reply({ error: "route inconnue" }, 404);
  } catch (e) {
    console.error(e);
    return reply({ error: "erreur serveur" }, 500);
  }
});
