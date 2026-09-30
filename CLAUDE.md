# AutoLad

App desktop de montage vidéo automatique, 100 % locale (aucun service cloud).
Distribuée sous forme d'installeur Windows (.exe / .msi).

## Stack

- **Shell** : Tauri 2
- **Back** : Rust + tokio
- **Front** : React + TypeScript (strict) + Vite, Tailwind + shadcn/ui
- **Vidéo** : ffmpeg / ffprobe embarqués en sidecar, build **GPL** statique de BtbN (n8.1.3, figé dans `scripts/fetch-ffmpeg.ps1`, SHA-256 vérifié). GPL choisi pour avoir libx264 en fallback ; ffmpeg tourne dans un process séparé (pas de liaison), mais la notice de licence est livrée (`third-party/ffmpeg/LICENSE.txt`) et il faut fournir la source ou une offre de source à la distribution.
- **IA** : whisper-rs (whisper.cpp) avec backend **Vulkan** par défaut (`cuda` optionnel), modèles ggml quantifiés téléchargés à la demande (défaut : `small` q5_1, ~190 Mo ; `large-v3-turbo` en option), hash vérifié
- **Agents** : le même `autolad.exe` lancé avec `--mcp` est un serveur MCP (stdio, sans fenêtre) : un seul exécutable à installer
- **Pont agent ↔ app** : l'app ouverte héberge elle-même le serveur MCP sur 127.0.0.1 (port + secret dans `<data>/bridge.json`). `--mcp` s'y connecte et ne fait que relayer stdin/stdout ; sans app ouverte (fichier absent, périmé ou secret refusé) il sert un moteur autonome. Même `Engine` pour l'UI et l'agent = même projet en direct. Chaque action d'agent est annoncée au front (événement `agent-activity`) : le curseur « Claude » glisse vers la cible pendant la pause de 900 ms que le back laisse avant d'agir, clique à la fin, puis l'UI recharge le projet. Pour que l'agent voie l'app, l'ouvrir AVANT de (re)connecter le MCP (`/mcp`).
- **Bindings TS** : générés depuis Rust avec tauri-specta (jamais écrits à la main)
- **Package manager** : pnpm

## Architecture

```
autolad/
├─ CLAUDE.md
├─ Cargo.toml                # workspace Rust (members: src-tauri, crates/*)
├─ package.json
├─ src/                      # front React
│  ├─ app/                   # bootstrap, providers, layout
│  ├─ features/              # un dossier par feature
│  │  ├─ library/            # import, liste des rushes
│  │  ├─ timeline/           # affichage/édition de l'EDL
│  │  ├─ preview/            # lecteur sur proxys
│  │  ├─ automation/         # réglages auto-montage (silences, scènes, transcription)
│  │  └─ export/             # rendu final
│  ├─ shared/                # ui (shadcn), hooks et utils génériques
│  └─ ipc/                   # bindings.ts généré + wrappers typés
├─ src-tauri/                # crate app : adaptateur Tauri, le plus fin possible
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ lib.rs              # composition root : câble core + impls
│  │  ├─ state.rs            # AppState (config, file de jobs, cache)
│  │  ├─ error.rs            # AppError sérialisable pour le front
│  │  └─ commands/           # un fichier par feature, zéro logique métier
│  ├─ binaries/              # ffmpeg-<target-triple>.exe, ffprobe-<target-triple>.exe
│  └─ tauri.conf.json
└─ crates/
   ├─ core/                  # domaine + logique métier, AUCUNE dépendance Tauri/ffmpeg
   ├─ media/                 # impl ffmpeg/ffprobe (process, parsing, filtergraph, encodeurs, hash)
   ├─ transcribe/            # impl whisper-rs (Vulkan/CUDA), catalogue et téléchargement des modèles
   └─ mcp/                   # moteur de montage + serveur MCP (rmcp) ; `engine` = logique, `server` = adaptateur
```

### Règles de dépendance (à respecter strictement)

- `core` ne dépend d'aucune infra. Il contient :
  - les types du domaine (`Project`, `Asset`, `Segment`, `Edl`, `Cut`…)
  - les algos purs (silences → segments, fusion, marges, construction de l'EDL)
  - les traits aux frontières I/O uniquement (`MediaProbe`, `Analyzer`, `Renderer`, `Transcriber`)
- `media` et `transcribe` implémentent les traits de `core` (`transcribe` réutilise `media` pour décoder l'audio).
- `mcp` compose `core` + `media` + `transcribe` sans Tauri ; `src-tauri` l'appelle via `run_mcp()` (`--mcp`). `engine.rs` porte tout le comportement et se teste sans transport ; `server.rs` ne fait que parser les arguments et mapper les erreurs.
- Sur stdout en mode `--mcp` ne passe **que** du JSON-RPC : jamais de `println!`.
- `src-tauri` câble le tout, expose les commands, gère les jobs et la progression. Une command = parser l'entrée → appeler `core`/un service → mapper l'erreur. Rien de plus.
- Le front ne parle au back **que** via `src/ipc`.

### Flux d'auto-montage

1. Import → ffprobe → hash → entrée en cache
2. En tâche de fond : proxy basse déf, waveform, vignettes
3. Analyse : silencedetect / détection de scène / transcription → `Vec<Segment>`
4. `core` construit une `Edl` à partir des analyses + réglages utilisateur
5. Le front affiche et édite l'EDL sur la timeline (preview sur proxys)
6. Export : EDL → graphe de filtres ffmpeg → rendu final sur les sources

## Conventions de code

### Général

- Code, identifiants et commentaires en anglais ; commentaires pour le *pourquoi*, pas le *quoi*.
- Pas d'abstraction prématurée : un trait seulement à une frontière I/O ou s'il existe réellement 2 implémentations (dont un mock de test).
- Fonctions courtes, noms explicites, pas de code mort ni de TODO sans ticket.

### Rust

- `rustfmt` + `clippy -D warnings`.
- Erreurs : `thiserror` dans chaque crate ; `AppError` sérialisable côté commands. Pas d'`anyhow` dans `core`.
- Pas d'`unwrap`/`expect` hors tests (sauf invariant documenté en commentaire).
- I/O et process en async (tokio) ; calcul CPU lourd dans `spawn_blocking` ou rayon. Ne jamais bloquer le runtime.
- ffmpeg lancé via `tokio::process`, progression lue avec `-progress pipe:1`.
- Chemins en `PathBuf`, jamais en `String`.

### TypeScript / React

- `strict: true`, pas de `any`.
- État éditeur : Zustand, un store par feature ; pas de store global fourre-tout.
- Composants petits, logique dans des hooks.
- Timeline rendue en `<canvas>`, pas un élément DOM par clip/frame.

## Performance

- Tout calcul lourd côté Rust, jamais dans la webview.
- Jobs longs dans une file avec annulation (`CancellationToken`) ; progression envoyée via `tauri::ipc::Channel`.
- Pas de gros JSON sur l'IPC : waveforms et vignettes transitent en binaire.
- Cache par hash (blake3 sur taille + premiers/derniers Mo) dans le dossier cache de l'app : proxys, waveforms, analyses, transcriptions. Ne jamais recalculer ce qui est en cache.
- Preview sur proxys (≈540p, GOP court pour un seek rapide) ; rendu final sur les sources.
- Encodage matériel (NVENC / QSV / AMF) détecté au démarrage, fallback libx264.
- Mesurer avant d'optimiser (criterion pour les algos de `core` si besoin).

## Commandes

```bash
pnpm install
pnpm tauri dev          # dev
pnpm tauri build        # installeur Windows
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
pnpm lint
pnpm typecheck
pnpm test               # vitest (front)
pwsh scripts/fetch-ffmpeg.ps1   # requis avant build/tests : installe les sidecars ffmpeg/ffprobe
cargo test -p autolad-transcribe --test whisper -- --ignored --nocapture   # télécharge un modèle (~190 Mo)
autolad.exe --mcp       # serveur MCP ; en dev : `cargo run -q -p autolad -- --mcp` (voir .mcp.json)
UPDATE_BINDINGS=1 cargo test -p autolad bindings   # regenerate src/ipc/bindings.ts
```

## Notes de build

- `tauri-specta` / `specta` sont épinglés en `=2.0.0-rc.21` / `=2.0.0-rc.22` : les rc.25 exigent un Rust plus récent que 1.90. À remonter en même temps que la toolchain.
- `typescript` est en 5.9 : `typescript-eslint` ne supporte pas encore TS 7.
- `src-tauri/build.rs` embarque `app.manifest` (comctl32 v6) sur toutes les cibles, sinon les binaires de test plantent au chargement (`STATUS_ENTRYPOINT_NOT_FOUND`).
- Le test `bindings_are_up_to_date` échoue si `src/ipc/bindings.ts` est périmé.
- `core` expose `specta::Type` derrière la feature `specta` (activée par `src-tauri` uniquement).
- `mcp` expose aussi `specta` (activée par `src-tauri`) : l'UI desktop réutilise `Engine` (import, silences, EDL, preview, rendu) via `src-tauri/src/commands/editor.rs`, donc une seule logique pour l'UI et pour les agents. Les indices/tailles 64 bits sont exportés en `number` (`BigIntExportBehavior::Number`).
- UI : `tauri-plugin-dialog` (officiel) pour les sélecteurs de fichiers : Importer (Browse…), Nouveau/Ouvrir/Enregistrer/Enregistrer sous (Ctrl+N/O/S, extension `.autolad`, JSON versionné) et destination de l'export ; il embarque `tauri-plugin-fs` en transitif, sans permission accordée. Le glisser-déposer (chemins réels via `onDragDropEvent`) et le chemin collé restent possibles ; sans destination, l'export écrit `<source>_autolad.mp4` à côté du premier rush. Preview : proxy H.264 540p (GOP 15) + AAC généré par asset dans `<data>/proxies/<id>.mp4`, joué par un `<video>` (son inclus) piloté par la timeline (`preview/useVideoMonitor.ts` ; la vidéo est l'horloge en lecture, saut de source à chaque fin de cut). En attendant le proxy, repli sur un PNG par position du playhead.
- Miniatures (`thumbs/<id>.jpg`, une bande de tuiles 78×44 tirée des keyframes du proxy) et pics audio (`waves/<id>.bin`, 100 octets/s, transmis en base64 par l'IPC) sont dessinés dans le canvas de la timeline. Tout est caché par id d'asset (hash) ; fichiers écrits en `.part` puis renommés.
- Protocole `asset://` : feature Cargo `protocol-asset` de `tauri` + scope `$APPLOCALDATA/{proxies,thumbs,waves}/**` dans `tauri.conf.json`. Le scope suit le dossier de données : `AUTOLAD_HOME` le contourne, ne pas l'utiliser pour lancer l'UI.
- Test e2e de l'UI sans écran : lancer l'exe avec `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` et piloter la webview en CDP (invoke des commandes, `Page.captureScreenshot`). Ne pas taper dans la fenêtre pendant le test : les raccourcis (Suppr, S…) modifient l'EDL.
- Prérequis Windows : CMake, LLVM (libclang, pour bindgen) et Vulkan SDK (`winget install Kitware.CMake LLVM.LLVM KhronosGroup.VulkanSDK`), plus MSVC. `LIBCLANG_PATH` est posé par `.cargo/config.toml`.
- `.cargo/config.toml` place `target-dir` en `C:/t` : whisper.cpp + Vulkan imbriquent des dossiers CMake qui dépassent 260 caractères, et le FileTracker de MSBuild ne gère pas les chemins longs (`FTK1011`). Marge faible : ne pas rallonger ce chemin.
- `whisper-rs` 0.16.0 : `set_abort_callback_safe` est bugué avec une closure nue (cast de pointeur incorrect, `whisper_full` échoue en -6). On passe un `Box<dyn FnMut() -> bool>` (voir `transcribe/src/whisper.rs`). À revoir à la mise à jour.
- Timestamps par mot (`word_timestamps`) : précis mais whisper.cpp peut perdre des mots (observé : « apprendre » absent). Utiliser le mode phrase pour le texte, le mode mot pour placer les coupes.
- Annuler = dropper le future : `kill_on_drop` tue ffmpeg ; l'inférence whisper s'arrête via le callback d'abandon.
- shadcn/ui : `components.json` est configuré, les composants s'ajoutent à la demande (`pnpm dlx shadcn add <nom>`).

## Git

- Remote : `origin` = `git@github.com:Sutsikii/autolad.git`. Branche par défaut : `main`.
- Commits Conventional Commits (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`, `ui:`…), message court **en anglais**, scope optionnel (`core`, `media`, `transcribe`, `mcp`, `tauri`, `front`).
- Aucune mention de Claude/Anthropic ni `Co-Authored-By` dans les commits.
- Commits atomiques : un changement logique par commit, `src/ipc/bindings.ts` régénéré avec le changement Rust qui l'a causé.
- Ne jamais commiter `target/`, `node_modules/`, `dist/`, les sidecars `src-tauri/binaries/*.exe`, les modèles Whisper.
- Commandes Claude (`.claude/commands/`) : `/commit`, `/review-commits` (fmt + clippy + tests + typecheck puis commit/push), `/detect-bugs`, `/changelog`.

## Workflow attendu

- Proposer un plan avant tout changement touchant plusieurs fichiers.
- Demander avant d'ajouter une dépendance.
- Toute logique de segmentation/EDL dans `core` avec tests unitaires.
- Avant de rendre la main : fmt, clippy, tests et typecheck au vert.
- Mettre à jour ce fichier quand une décision d'architecture change.

## Décisions ouvertes

- Premier cas d'usage du MVP : le moteur couvre les silences d'une face cam ; highlights et templates restent à décider.
- Détection de scènes et assemblage multi-rushes évolué (le rendu accepte déjà plusieurs assets).
- Fichiers audio seul (sans piste vidéo) : refusés à l'import pour l'instant.
- Renouvellement du tag BtbN dans `fetch-ffmpeg.ps1` quand il est purgé (mettre à jour tag, nom de fichier et SHA-256 ensemble).

## Décisions prises

- Onglet Transcription (dock de droite, à côté d'Auto-cut) : transcription locale du clip sélectionné (langue + modèle au choix, phrases cliquables qui placent le playhead sur la timeline, phrase en cours surlignée pendant la lecture, copie du texte). Les transcriptions vivent dans le fichier projet ; `cached_transcript` les relit sans lancer de job. Quand un agent appelle `transcribe`, l'onglet passe au premier plan. Pas d'annulation ni de progression du téléchargement de modèle pour l'instant (le premier usage d'un modèle le télécharge).

- Toutes tailles de vidéo : `MediaInfo.width/height` = taille *affichée* (rotation smartphone et pixels anamorphiques appliqués, cf. `probe.rs`) ; proxy, PNG de preview et rendu passent par `scale=iw*sar:ih` (pixels carrés) et des côtés pairs. Export par défaut = taille du premier clip ; brouillon = côté le plus long ≤ 640 (un portrait devient 360×640). Encodeur matériel seulement entre 256×144 et 4096, libx264 sinon, et repli automatique sur libx264 si un encodeur matériel échoue. Vignettes : largeur de tuile proportionnelle à la forme de l'image (`tile_width`, dans le nom du fichier en cache). Le cadre du moniteur a la forme de la séquence (premier clip) ; les clips d'une autre forme y sont encadrés de barres, comme au rendu. Cas couverts par le test `every_video_shape_is_imported_previewed_and_exported_at_the_right_size` (paysage, portrait, pivoté, anamorphique, minuscule, impair, ultra-large, UHD).

- Clips sans piste audio (captures d'écran…) : acceptés (`Asset.has_audio`). Pas de silences/transcription/forme d'onde pour eux (erreur explicite qui renvoie vers `edit_edl insert`), piste A1 marquée « no audio », et au rendu un silence est généré (`anullsrc`) pour garder un flux audio par cut dans le `concat`.

- Undo/redo : l'`Engine` garde un historique de l'EDL (`core::history`, snapshots, 200 max, session seulement, remis à zéro par Nouveau/Ouvrir). Toute modif d'EDL passe par `State::commit` avec un libellé (`describe_ops`) ; une modif sans effet n'est pas enregistrée. UI (Ctrl+Z, Ctrl+Shift+Z/Ctrl+Y, boutons de la timeline) et agent (outils `undo`/`redo`) partagent le même historique ; `EdlSummary.history` dit ce qu'annuler/rétablir ferait. Les imports ne s'annulent pas.

- Fichier projet : JSON versionné (`version: 1`), écrit atomiquement, autosauvegardé une fois lié par `save_project`. L'UI et l'agent partagent le même fichier lié ; « Nouveau » le délie (plus aucune écriture vers l'ancien fichier).
- Modèle Whisper par défaut : `small` q5_1. Backend GPU : Vulkan.
- ffmpeg : build GPL statique BtbN en sidecar.
