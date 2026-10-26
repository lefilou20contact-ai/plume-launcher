package dev.plume.cosmetics.mixin;

import com.mojang.authlib.GameProfile;
import dev.plume.cosmetics.CosmeticsCache;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.client.resources.PlayerSkin;
import net.minecraft.resources.ResourceLocation;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Remplace la cape d'un joueur par celle choisie dans Plume ; le rendu reste celui du jeu. */
@Mixin(PlayerInfo.class)
public abstract class PlayerInfoMixin {
    @Shadow
    public abstract GameProfile getProfile();

    @Inject(method = "getSkin", at = @At("RETURN"), cancellable = true)
    private void plume$cape(CallbackInfoReturnable<PlayerSkin> cir) {
        ResourceLocation cape = CosmeticsCache.capeFor(getProfile().getId());
        if (cape == null) return;
        PlayerSkin skin = cir.getReturnValue();
        cir.setReturnValue(new PlayerSkin(skin.texture(), skin.textureUrl(), cape, skin.elytraTexture(), skin.model(), skin.secure()));
    }
}
