---
description: Commit les changements en cours sans mention de Claude
---

Committe les changements actuels du projet (staged + unstaged pertinents), avec un message de commit court, en anglais, au format Conventional Commits (`feat: add X`, `fix: handle Y`, `chore:`, `docs:`, `refactor:`, `test:`, `ui:`…) décrivant le "pourquoi" du changement. Toujours en anglais, jamais en français.

Un scope est bienvenu quand il clarifie la zone touchée : `core`, `media`, `transcribe`, `mcp`, `tauri`, `front` (ex : `feat(core): merge overlapping silences`).

Règles strictes :
- N'ajoute JAMAIS de ligne `Co-Authored-By: Claude`, `Claude-Session:`, ni aucune autre mention de Claude, Anthropic ou d'un outil d'IA dans le message de commit.
- Le message doit se lire comme s'il avait été écrit par l'utilisateur lui-même.
- Ne commit que les fichiers pertinents pour le changement (évite `git add -A` aveugle) ; vérifie `git status`/`git diff` avant de committer.
- Ne commit jamais `target/`, `node_modules/`, `dist/`, les sidecars `src-tauri/binaries/*.exe` ni les modèles Whisper (déjà dans `.gitignore`).
- Si `src/ipc/bindings.ts` est régénéré, il part dans le même commit que le changement Rust qui l'a provoqué.
- Ne push pas automatiquement, sauf demande explicite.
- Si des changements semblent sans rapport entre eux, propose de les séparer en plusieurs commits plutôt que d'en faire un seul fourre-tout.
