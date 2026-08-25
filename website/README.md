# Cymatrox — site

## État

Ce site affiche **exclusivement des résultats réels** du crate `cymatrox` — aucun
calcul simulé, aucun démo JS, aucun fallback approximatif.

### Mode serveur live (recommandé)

Un serveur local (`tools/cymatrox-live`) exécute de vraies simulations GPU via le
crate publié (`cymatrox 0.1` sur crates.io) et expose une API HTTP. Le site y
pense automatiquement :

- **au chargement** : premier calcul du module courant
- **à chaque mouvement de curseur** : anti-rebond 450 ms, dernier gagnant
- **à chaque changement d'onglet** : calcul du nouveau module

```sh
cd tools/cymatrox-live
cargo run --release          # http://127.0.0.1:8030
```

Chaque calcul dure typiquement 0,1 – 2 s selon le module et les paramètres
(acoustique 32³ le plus long, ~0,5 s). Les résultats sont servis en JSON
identique au format de `export_frames.rs`.

Le navigateur affiche un indicateur « recalcul GPU… » pendant le calcul
et l'ancien résultat reste visible jusqu'à l'arrivée du nouveau.

### Mode hors-ligne (replay)

Sans serveur local, le site lit les datasets exportés embarqués dans
`website/data/*.json.gz` (~620 Ko). Comportement et régénération :

```sh
cargo run --release --example export_frames   # écrit website/data/*.json
gzip -kf website/data/*.json                  # le site préfère les .gz
python3 -m http.server -d website 8000        # http://localhost:8000
```

En mode hors-ligne, les curseurs mettent à jour uniquement l'extrait de
config Rust à copier-coller dans votre propre projet.

## Structure

```
index.html / style.css / script.js   — le site
data/                                — frames exportées (*.json.gz committés)
examples/export_frames.rs            — producteur statique (racine du dépôt)
tools/cymatrox-live/                 — serveur live (API + fichiers statiques)
  Cargo.toml / src/main.rs
```

## Ce que les curseurs font

+------------------+--------------------------------------------+---------------------------+
| Contexte         | Frequence / modes n,m                      | Resolution grid           |
+------------------+--------------------------------------------+---------------------------+
| Serveur live     | Parametres du prochain calcul GPU          | Taille de la grille       |
| Hors-ligne       | Extrait de config Rust (copier-coller)     | Valeur affichee uniquement|
+------------------+--------------------------------------------+---------------------------+
