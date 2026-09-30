---
description: Résume les derniers changements d'AutoLad pour le propriétaire (pas un changelog public)
---

Génère un résumé en Markdown de ce qui s'est passé sur le projet, destiné au propriétaire (l'utilisateur lui-même) pour qu'il sache ce qui a changé — pas des release notes publiques.

## 1. Déterminer les commits à inclure

1. `git fetch origin` puis regarde `git log origin/main..HEAD --oneline`.
2. Si cette liste n'est pas vide, utilise ces commits (ce qui n'a pas encore été poussé).
3. Si elle est vide (tout est déjà pushé), demande à l'utilisateur quelle plage couvrir (ex: "depuis quand ?" / "combien de derniers commits ?") plutôt que de deviner — sauf s'il a déjà précisé une période/un nombre dans sa demande, auquel cas utilise ça directement.
4. Inclus TOUS les commits de la plage, y compris les changements internes (build, config, dépendances, bindings, tests) — le propriétaire veut savoir tout ce qui se passe, pas seulement ce qui est visible dans l'app. Ne filtre rien sur ce critère.

## 2. Rédiger le contenu

- Une ligne par commit (ou groupe de commits liés), en français, qui explique concrètement ce qui a changé et pourquoi (impact réel : nouvelle capacité de montage, bug corrigé, gain de perf, risque évité, etc.) — évite le jargon inutile mais un terme technique précis (ex: "EDL", "silencedetect", "proxy", "serveur MCP", "sidecar ffmpeg") est bienvenu si ça aide à comprendre où ça se passe.
- Précise si un changement est interne / invisible pour l'utilisateur final, si c'est pertinent.
- Regroupe par nature si utile (nouveautés / corrections / technique), mais reste minimaliste pour 2-3 items.
- Un emoji par ligne max, sobre (✨ nouveauté, 🐛 correction, ⚡ amélioration, 🔧 technique/interne) — n'en mets pas si ça n'apporte rien.
- Titre court avec la date du jour.
- Pas de blabla, pas de lien vers des commits/PR ; les références techniques utiles (nom de crate, fichier, zone du code) sont OK si elles aident à situer le changement.

## 3. Format de sortie

Le résultat DOIT être un seul bloc de code Markdown (```` ```md ... ``` ````) contenant uniquement le texte du résumé — rien d'autre dedans. Titre court avec la date, puis une liste à puces.

Après le bloc de code, ajoute une seule ligne en dehors du bloc précisant combien de commits ont été résumés, rien de plus (pas de récap détaillé, pas de proposition de push).
