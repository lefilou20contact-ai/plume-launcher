# Traductions de Plume

- `fr.json` est la **langue de référence** : toutes les clés y sont. Les autres langues sont comparées à lui.
- Une clé vide (`""`) ou absente = non traduite. Elle s'affiche alors en français.
- Le **pourcentage** d'une langue est calculé tout seul (clés traduites / clés du français), dans le launcher et avec `node scripts/i18n.js status`.

## Synchronisation automatique
Les langues sont récupérées depuis ce dossier sur GitHub (branche `main`) au démarrage du launcher : **un simple push suffit, aucune nouvelle version n'est nécessaire**. Les launchers affichent « Traductions mises à jour » et le pourcentage se recalcule.
Un texte dont les variables `{x}` diffèrent du français est ignoré par le launcher (il garde alors le français pour cette phrase).

## Ajouter une langue
    node scripts/i18n.js new de "Deutsch"     # crée src/i18n/de.json (vide) et l'ajoute à languages.json
    node scripts/i18n.js missing de            # liste ce qu'il reste à traduire
    node scripts/i18n.js check                 # vérifie les variables {comme_ceci} et le JSON

## Règles
- Garde les variables telles quelles : `{name}`, `{n}`, `{v}`…
- Pluriels : `un mod|{n} mods` (avant le `|` = singulier, après = pluriel).
- Les clés `rs.*` et `pg.*` sont les messages venant du programme ; ne change pas leur version française.
