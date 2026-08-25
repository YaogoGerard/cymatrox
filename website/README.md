# Cymatrox — site

## État réel de ce paquet

- **Le site fonctionne en mode « données réelles »** : les onglets Granular /
  Fluid / Acoustic lisent des frames exportées par le vrai crate `cymatrox`
  v0.1.0 (`website/data/*.json.gz`, ~620 Ko au total, compressé gzip) et les
  animent en 2D (canvas) comme en 3D (Three.js : nuage de points pour les
  grains, maillage déformé coloré pour la surface liquide, plan texturé par
  la pression acoustique au plan médian z).
- Les données sont régénérables à volonté :
  ```sh
  cargo run --release --example export_frames   # écrit website/data/*.json
  gzip -kf website/data/*.json                  # le site préfère .json.gz
  ```
  Seuls les `.gz` sont committés (`.gitignore` exclut les `.json` bruts).
- **Serve requis en HTTP** (le fetch de fichiers locaux ne passe pas en
  `file://`) :
  ```sh
  cd website
  python3 -m http.server 8000    # http://localhost:8000
  ```
  En repli hors-ligne totale : bouton de chargement de fichier intégré
  (glisser un `.json` ou `.json.gz` via l'input caché — voir `index.html`,
  `#data-file`). Décompression navigateur native via `DecompressionStream`.
- **`wasm-wrapper/`** contient un pont Rust réel (wasm-bindgen) écrit contre
  l'API publiée de `cymatrox` v0.1.0. **Il n'a jamais été compilé** et son
  usage en navigateur se heurte au readback bloquant de la crate
  (`poll(Wait)` — impossible sous WebGPU, cf. ADR-0006 vs contraintes du
  navigateur). Il est conservé tel quel comme base d'expérimentation
  future ; `script.js` le détecte automatiquement s'il est compilé et sert
  alors de simulateur live lorsque des données exportées sont absentes.
- La vue héro (canvas d'en-tête) reste une approximation Chladni en JS pur,
  volontairement décorative.

## Structure

```
index.html / style.css / script.js   — le site (données réelles + replis)
data/                                — frames exportées (*.json.gz committés)
examples/export_frames.rs            — producteur (racine du dépôt)
wasm-wrapper/                        — pont Rust → WASM (expérimental)
  Cargo.toml
  src/lib.rs
  BUILD.md                           — comment compiler + dépanner
```

## Ce que les curseurs font en mode données réelles

Les curseurs (fréquence, modes n/m, résolution) ne modifient pas la lecture
— ils recalculent l'extrait de config Rust exporté en bas du panneau, pour
copier-coller vers `cymatrox` dans ton propre projet. Le replay reste fidèle
aux paramètres figés lors de l'export (440 Hz granulaire, 60 Hz fluide,
24 kHz acoustique).
