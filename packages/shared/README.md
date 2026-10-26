# @signet/shared

Du code TypeScript **sans plateforme**, commun à l'app de bureau (Electron) et à l'app mobile (React Native / Expo). C'est un paquet « source » : pas d'étape de compilation, les apps importent directement les fichiers `.ts`.

## Ce qu'il y a dedans

| Module             | Contenu                                                                                                                                                                                                                            |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `types/`           | Le modèle de données : onglets, espaces, signets, réglages, historique, mots de passe, outils de développement.                                                                                                                    |
| `model.ts`         | Fabriques et assainisseurs du modèle (`newSpace`, `newTab`, `cleanSpaceDraft`).                                                                                                                                                    |
| `tree.ts`          | Utilitaires d'arbre : listes d'onglets, dossiers, vues partagées, groupes.                                                                                                                                                         |
| `tab-list.ts`      | Réducteur d'une liste d'onglets à plat avec un onglet actif (utilisé par le mobile).                                                                                                                                               |
| `split.ts`         | Vue partagée : calcul des rectangles et des séparateurs, insertion d'un onglet.                                                                                                                                                    |
| `urls.ts`          | Saisie vers adresse ou recherche (`toUrl`, `searchUrl`).                                                                                                                                                                           |
| `site.ts`          | `siteOf` : le domaine d'un site (liste des suffixes publics).                                                                                                                                                                      |
| `routes.ts`        | Règles d'aiguillage des liens vers un espace.                                                                                                                                                                                      |
| `feeds.ts`         | Lecture des flux RSS et Atom (dossiers dynamiques).                                                                                                                                                                                |
| `ids.ts`           | `newId` : un identifiant unique, avec repli si `crypto.randomUUID` manque.                                                                                                                                                         |
| `i18n.ts`          | Création de l'instance i18next (`createI18n`), choix de la langue (`resolveLocale`).                                                                                                                                               |
| `mobile/`          | Logique de l'app mobile : `workspace` (espaces et onglets), `history`, `bookmarks`, `settings`, `search`, `suggestions`, `color` (teintes des espaces), `persist` (document enregistré, versions, migrations). Un test par module. |
| `locales/{en,fr}/` | Les catalogues de textes : `main.json` (bureau, processus principal), `ui.json` (bureau, interface), `mobile.json`.                                                                                                                |

## La règle

- **Ici, seulement du code qui marche partout** : dans le processus principal d'Electron, dans l'interface web, et dans Hermes (le moteur JavaScript de React Native).
- **Jamais d'import de** `electron`, `node:*`, du DOM (`document`, `window`…), ni de `react-native`. Le `tsconfig` du paquet n'a ni les types DOM ni les types Node (`"types": []`, `lib: ES2023`) : un import interdit fait échouer `npm run typecheck`.
- **Ne dépend d'aucune app** : ne jamais importer `app/desktop` ni `app/mobile`.
- **Ce qui est propre à une plateforme est injecté** par l'appelant : les noms par défaut traduits, les identifiants… (voir `newFolder` dans `tree.ts` et `cleanSpaceDraft` dans `model.ts`).
- **Le seul global hors ECMAScript** dont le paquet a besoin est `URL`. `src/globals.d.ts` le déclare (c'est un fichier de types, il ne change rien à l'exécution). Chez les consommateurs, les types DOM ou Node fournissent déjà `URL` : le bureau exclut donc ce fichier de son `tsconfig.web.json`.

## Comment s'en servir

On importe par sous-chemin, jamais depuis un index général :

```ts
import { siteOf } from '@signet/shared/site'
import { toUrl } from '@signet/shared/urls'
import type { TabInfo } from '@signet/shared/types'
import { openTab } from '@signet/shared/mobile/workspace'
import en from '@signet/shared/locales/en/ui.json'
```

Les correspondances sont dans `package.json` (`exports`) : `./types` pointe sur `src/types/index.ts`, `./locales/*` sur `src/locales/*`, `./*` sur `src/*.ts`.

## Ajouter un module

1. Créez `src/mon-module.ts`. Il est importable tout de suite sous `@signet/shared/mon-module`.
2. Écrivez des **fonctions pures** : mêmes entrées, mêmes sorties, rien qui touche au disque, au réseau ou à l'écran.
3. Écrivez son test à côté : `src/mon-module.test.ts` (Vitest).
4. Lancez `npm test -w @signet/shared`, puis `npm run typecheck` depuis la racine.
5. Ajoutez une ligne au tableau ci-dessus.

Pour un texte visible, ajoutez la clé dans les **deux** langues, puis lancez `npm run i18n:check`.

## À ne pas confondre

`app/desktop/src/shared` (alias `@shared/*`) est un **autre** dossier « shared » : il contient le code commun aux processus du **bureau seulement** (contrat IPC, raccourcis, appareils de test, types de fenêtres et d'extensions). Le mobile ne le voit pas. Pour choisir : si le mobile peut en avoir besoin, c'est ici ; sinon, c'est là-bas.

Le bureau réexporte les types de ce paquet depuis `@shared/types`, pour que son code continue d'importer tous ses types au même endroit.

## Tests

Les tests sont à côté du code (`*.test.ts`). Pour les lancer : `npm test -w @signet/shared`. La carte du code est dans [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md).
