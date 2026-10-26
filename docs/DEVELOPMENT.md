# Développement

Toutes les commandes se lancent depuis la racine du dépôt (workspaces npm : `app/desktop`, `app/mobile`, `packages/shared`).

## Lancer

| Commande                                         | Effet                                                                        |
| ------------------------------------------------ | ---------------------------------------------------------------------------- |
| `npm run dev`                                    | Bureau en développement (electron-vite, rechargement à chaud).               |
| `npm run build` / `npm start`                    | Construit dans `app/desktop/out/`, puis lance cette version.                 |
| `npm test`                                       | Tests unitaires (Vitest) du bureau et de `packages/shared`.                  |
| `npm run test:e2e`                               | Tests de bout en bout (Playwright + Electron), voir plus bas.                |
| `npm run typecheck`                              | `tsc` sur le bureau (main, renderer, e2e), le partagé et le mobile.          |
| `npm run lint`                                   | ESLint.                                                                      |
| `npm run format` / `npm run format:check`        | Prettier (écrit / vérifie).                                                  |
| `npm run i18n:check`                             | Vérifie que `fr` et `en` ont les mêmes clés.                                 |
| `npm run icons`                                  | Régénère les icônes de l'app.                                                |
| `npm run dist`, `dist:linux`, `dist:win`         | Paquet macOS, AppImage Linux, installeur Windows (dans `app/desktop/dist/`). |
| `npm run mobile`, `mobile:ios`, `mobile:android` | Expo (app mobile).                                                           |

À ne pas faire : lancer `electron-vite build` dans `out/` pendant que `npm run dev` tourne. Le build réécrit les fichiers que l'app en cours lit, et l'Electron orphelin ne se laisse plus quitter. C'est pour cela que les tests e2e construisent dans `out-e2e/`.

## Tests

- **Unitaires** : un fichier `*.test.ts` à côté du code qu'il teste (`src/shared`, `src/main`, `src/renderer`). Le modèle d'onglets (`src/main/model/`) se teste sans Electron avec `model/testkit.ts` (`world()` : un état minimal, les effets enregistrés, `shape()` pour écrire une liste en une ligne). Les modules qui touchent à Electron le simulent avec `vi.mock('electron', …)`.
- **Bout en bout** : `app/desktop/tests/e2e/*.spec.ts`, un seul worker, un profil temporaire et une instance d'Electron par test.
  - `npm run test:e2e` construit l'app dans `app/desktop/out-e2e/` puis lance Playwright. Options Playwright après `--` : `npm run test:e2e -- -g palette`.
  - `SIGNET_E2E_SKIP_BUILD=1` réutilise le dernier build de `out-e2e/` (utile quand seuls les tests changent).
  - `SIGNET_E2E_MAIN=<fichier>` lance un autre point d'entrée que `out-e2e/main/index.js`.
  - `SIGNET_E2E=1` (posé par `fixtures.ts`) fait exposer `globalThis.__signet`, le `Browser` du process principal, jamais dans l'app empaquetée. Dans un test, `signetApp.signet((b, arg) => …, arg)` exécute une fonction dans le main avec ce `Browser` ; `signetApp.snapshot()` renvoie l'état envoyé à l'interface.
  - Fixtures : `launch(profil)` (relance sur un profil existant), `openTab`, `uiPage(app, '#overlay')` (vue d'interface par fragment d'URL), `quit`, `site.url('nom')` (serveur local dont la page a pour titre `nom`).
  - Attendre une condition avec `expect.poll(...)`, pas avec des pauses fixes.

## Variables d'environnement `SIGNET_*`

Les outils de développement vivent dans `app/desktop/src/main/dev/` (`seed.ts`, `capture.ts`, `probes.ts`, entrée `index.ts`). `main/index.ts` les importe seulement si l'app n'est pas empaquetée ; ils forment un chunk `out/main/dev-*.js` exclu du paquet par `electron-builder.mjs`.

Les variables d'état d'exemple (`SIGNET_SEED` et ses options) ne s'appliquent qu'avec `--user-data-dir=<dossier>` : jamais sur votre vrai profil. Les arguments après `--` vont à Electron. Une capture complète :

```
SIGNET_SEED=1 SIGNET_THEME=dark SIGNET_CAPTURE=main:/tmp/main.png npm run dev -w @signet/desktop -- -- --user-data-dir=/tmp/signet-profil
```

La capture est un PNG à chemin absolu ; l'app quitte ensuite toute seule. Cibles : `main`, `settings`, `settings:<section>`, `library`.

### État d'exemple et apparence

| Variable                         | Effet                                                                                                                                   | Exemple                              |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `SIGNET_THEME`                   | Force le thème `light` ou `dark`.                                                                                                       | `SIGNET_THEME=dark`                  |
| `SIGNET_SEED`                    | Remplace l'état en mémoire par un état d'exemple (2 espaces, favoris, dossiers, archive, règles d'aiguillage). Exige `--user-data-dir`. | `SIGNET_SEED=1`                      |
| `SIGNET_ACTIVE`                  | Onglet actif au départ : celui dont l'URL contient ce texte.                                                                            | `SIGNET_ACTIVE=github`               |
| `SIGNET_URL`                     | Remplace l'adresse de l'onglet actif.                                                                                                   | `SIGNET_URL=http://localhost:5173/`  |
| `SIGNET_SPLIT`, `SIGNET_SPLIT_N` | Vue partagée des premiers onglets du jour : `columns`, `rows` ou `grid`, sur 2 à 4 onglets.                                             | `SIGNET_SPLIT=grid SIGNET_SPLIT_N=3` |
| `SIGNET_SIDEBAR`                 | `0` : barre latérale masquée.                                                                                                           | `SIGNET_SIDEBAR=0`                   |
| `SIGNET_LAYOUT`                  | `horizontal` : onglets en haut, avec deux téléchargements factices.                                                                     | `SIGNET_LAYOUT=horizontal`           |
| `SIGNET_TABS`                    | Nombre total d'onglets du jour (complète avec des exemples).                                                                            | `SIGNET_TABS=30`                     |
| `SIGNET_SPACES`                  | Nombre d'espaces au total (complète avec des espaces vides).                                                                            | `SIGNET_SPACES=8`                    |
| `SIGNET_ACTIVE_SPACE`            | Indice de l'espace actif au départ.                                                                                                     | `SIGNET_ACTIVE_SPACE=1`              |

### Capture d'écran

| Variable            | Effet                                                                                                                                 | Exemple                                      |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `SIGNET_CAPTURE`    | `<cible>:<chemin.png>` : ouvre la fenêtre, attend, enregistre une capture (vues web composées) et quitte.                             | `SIGNET_CAPTURE=settings:privacy:/tmp/s.png` |
| `SIGNET_WAIT`       | Délai en ms avant la capture de la fenêtre principale (3500 par défaut).                                                              | `SIGNET_WAIT=8000`                           |
| `SIGNET_COMPACT`    | `1` : mode compact.                                                                                                                   | `SIGNET_COMPACT=1`                           |
| `SIGNET_FLOAT`      | `toolbar` : barre d'outils flottante affichée, capturée à part (`-float.png`).                                                        | `SIGNET_FLOAT=toolbar`                       |
| `SIGNET_MEDIA`      | `1` : carte média factice dans la barre latérale.                                                                                     | `SIGNET_MEDIA=1`                             |
| `SIGNET_SITE`       | `1` : ouvre le panneau du site, capturé à part (`-site.png`).                                                                         | `SIGNET_SITE=1`                              |
| `SIGNET_SPACE_FORM` | `1` : formulaire d'espace ouvert.                                                                                                     | `SIGNET_SPACE_FORM=1`                        |
| `SIGNET_POPOVER`    | `spaces`, `folder`, `space-form` ou `downloads` : popover de la rangée d'onglets (avec `SIGNET_LAYOUT=horizontal`).                   | `SIGNET_POPOVER=downloads`                   |
| `SIGNET_SWIPE`      | De -1 à 1 : glissement entre espaces figé à mi-course.                                                                                | `SIGNET_SWIPE=0.5`                           |
| `SIGNET_DEVTOOLS`   | `right`, `bottom` ou `window` : outils de développement de l'onglet actif (la fenêtre détachée est capturée à part, `-devtools.png`). | `SIGNET_DEVTOOLS=right`                      |

### Sondes

| Variable              | Effet                                                                                                      | Exemple                                               |
| --------------------- | ---------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| `SIGNET_OPEN`         | Ouvre l'adresse dans un onglet actif de la fenêtre principale.                                             | `SIGNET_OPEN=https://example.com/`                    |
| `SIGNET_OPEN_BG`      | Avec `SIGNET_OPEN` : ouvre d'abord en arrière-plan, affiche après ce délai (ms).                           | `SIGNET_OPEN_BG=3000`                                 |
| `SIGNET_EVAL`         | Script lancé dans l'onglet actif avant la capture ; résultat affiché (`EVAL`).                             | `SIGNET_EVAL=document.title`                          |
| `SIGNET_UIJS`         | Script (async) lancé dans l'interface avant la capture ; résultat affiché (`UIJS`).                        | `SIGNET_UIJS=document.title`                          |
| `SIGNET_CONSOLE`      | `1` : recopie les avertissements et erreurs de console de toutes les pages (`CONSOLE`).                    | `SIGNET_CONSOLE=1`                                    |
| `SIGNET_DEVELOP`      | JSON `DevOverrides` appliqué à l'onglet actif (mode développeur), puis ce que la page en voit (`DEVELOP`). | `SIGNET_DEVELOP='{"scheme":"dark","noJs":true}'`      |
| `SIGNET_DEVELOP_WAIT` | Délai en ms avant cette lecture (4500 par défaut).                                                         | `SIGNET_DEVELOP_WAIT=8000`                            |
| `SIGNET_INSTALL_EXT`  | Identifiants de la boutique à installer (séparés par des virgules), puis état des extensions (`EXTCHECK`). | `SIGNET_INSTALL_EXT=cjpalhdlnbpafiamejdnhcphjbkeiagm` |
| `SIGNET_EXT_SET`      | JSON de réglages appliqué avant la capture (ordre, masquage des extensions).                               | `SIGNET_EXT_SET='{"extensionOrder":[]}'`              |

### Hors de `dev/`

Ces variables sont lues par du code qui existe aussi dans l'app packagée.

| Variable             | Effet                                                                                                       | Exemple                        |
| -------------------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------ |
| `SIGNET_SAFE_MODE`   | `1` : démarre sans mods (équivalent de `--safe-mode`), si un mod rend l'interface inutilisable.             | `SIGNET_SAFE_MODE=1`           |
| `SIGNET_SWIPEDIAG`   | `1` : le script de balayage des pages consigne ses décisions dans la console de la page.                    | `SIGNET_SWIPEDIAG=1`           |
| `SIGNET_WEBAPPS_DIR` | Remplace le dossier des lanceurs d'apps web (`~/Applications/Signet Apps`).                                 | `SIGNET_WEBAPPS_DIR=/tmp/apps` |
| `SIGNET_AUTH_REASON` | Interne (Windows) : texte passé à PowerShell pour la fenêtre d'authentification. Ne se règle pas à la main. |                                |

### Tests

| Variable                | Effet                                                                                                       | Exemple                                    |
| ----------------------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| `SIGNET_E2E`            | `1` : expose `globalThis.__signet` et coupe le bloqueur de pubs (hors app packagée). Posé par les fixtures. |                                            |
| `SIGNET_E2E_MAIN`       | Point d'entrée à lancer (par défaut `out-e2e/main/index.js`).                                               | `SIGNET_E2E_MAIN=out/main/index.js`        |
| `SIGNET_E2E_SKIP_BUILD` | `1` : ne reconstruit pas `out-e2e/` avant les tests.                                                        | `SIGNET_E2E_SKIP_BUILD=1 npm run test:e2e` |

## Contrôles manuels

Les sondes qui exigeaient un vrai clavier ou un vrai site (⌘Q maintenu et sa bulle, mini-lecteur sur YouTube, glisser réel sur les boutons d'extension) ont été retirées. Ce qui reste à contrôler à la main est dans [A-VERIFIER.md](A-VERIFIER.md); pensez à y ajouter ces cas.
