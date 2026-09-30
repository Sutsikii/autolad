---
description: Détecte les bugs potentiels dans le code modifié (Rust / Tauri / React)
---

Analyse le code modifié ou le fichier ouvert dans l'IDE pour détecter les bugs potentiels. Suis ces étapes dans l'ordre :

## 1. Identifier le périmètre d'analyse

- Si un fichier est sélectionné ou ouvert dans l'IDE, analyse ce fichier en priorité
- Sinon, utilise `git diff HEAD` pour obtenir tous les fichiers modifiés depuis le dernier commit
- Liste les fichiers qui seront analysés avant de commencer

## 2. Analyse statique du code

Pour chaque fichier, cherche activement ces catégories de bugs :

### Bugs logiques (Rust)
- Conditions inversées, bornes off-by-one sur les plages de temps (segments, marges, fusion de silences)
- Conversions `as` qui tronquent (f64 → u64, i64 → usize) ou perdent du signe
- Comparaisons de `f64` avec `==`, valeurs `NaN`/négatives non gérées dans les durées et timestamps
- `unwrap`/`expect`/indexation `[i]` hors tests qui peuvent paniquer
- Segments vides, EDL vide, asset sans durée : cas limites non traités

### Bugs async / process
- Calcul CPU lourd ou I/O bloquante dans le runtime tokio (hors `spawn_blocking`/rayon)
- Lock `std::sync::Mutex` tenu à travers un `.await`
- Process ffmpeg non tué à l'annulation (`kill_on_drop` manquant), pipes stdout/stderr non drainés (deadlock)
- `CancellationToken` ignoré dans une boucle longue, progression jamais terminée
- Chemins passés en `String` au lieu de `PathBuf`, échappement d'arguments ffmpeg / filtergraph

### Bugs d'architecture
- `core` qui dépend de Tauri, ffmpeg ou d'une infra
- Logique métier dans `src-tauri/src/commands/`
- `println!` ou écriture sur stdout dans le chemin `--mcp` (casse le JSON-RPC)
- `src/ipc/bindings.ts` écrit à la main ou périmé

### Bugs de sécurité
- Injection dans les arguments de process (chemin ou texte utilisateur dans un filtergraph)
- Téléchargement de modèle / sidecar sans vérification de hash
- Capabilities Tauri trop larges, accès disque non borné

### Bugs TypeScript / React
- `any`, `as` forcé, `!` non justifié
- `useEffect` sans cleanup, dépendances manquantes, état mis à jour après démontage
- Un store Zustand global fourre-tout, logique métier dans un composant au lieu d'un hook
- Timeline : un élément DOM par clip/frame au lieu du `<canvas>`
- Gros JSON transitant sur l'IPC (waveforms/vignettes doivent être binaires)
- Keys manquantes ou non uniques dans les listes `.map()`

## 3. Rapport des bugs trouvés

Pour chaque bug détecté, présente :

```
🐛 [SÉVÉRITÉ: CRITIQUE | ÉLEVÉE | MOYENNE | FAIBLE]
Fichier : chemin/vers/fichier.rs (ligne X)
Type : (ex: Panic possible, Deadlock, Off-by-one, Faille sécurité...)
Problème : Description claire du bug et pourquoi c'est un problème
Code actuel :
  [extrait du code problématique]
Correction suggérée :
  [code corrigé]
```

## 4. Appliquer les corrections

- Demande confirmation avant d'appliquer les corrections CRITIQUES et ÉLEVÉES
- Applique directement les corrections MOYENNES et FAIBLES
- Toute correction de logique de segmentation/EDL s'accompagne d'un test unitaire dans `core`
- Après correction, re-vérifie que le fix n'introduit pas de nouveau bug (`cargo clippy`, `cargo test`, `pnpm typecheck`)

## 5. Résumé final

Termine avec un tableau récapitulatif :
- Nombre de bugs trouvés par sévérité
- Nombre de bugs corrigés automatiquement
- Fichiers modifiés

Si aucun bug n'est trouvé, dis-le clairement avec les éléments vérifiés.
