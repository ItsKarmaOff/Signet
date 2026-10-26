# Signet — spécification v1

Navigateur macOS en Electron, organisé autour d'une barre latérale verticale.
L'interface est en **français**.

Stack : Electron 45, electron-vite 6, React 19, TypeScript 6, lucide-react pour les icônes, @ghostery/adblocker-electron pour le bloqueur de pubs, emojibase-data pour le sélecteur d'emoji.

Le contrat entre les processus est dans `app/desktop/src/shared/types/` (`api.ts` pour `SignetApi`) et `app/desktop/src/shared/ipc/contract.ts` (canaux). **Ne pas le modifier** sans le signaler.
Alias d'import : `@shared/types`. Les types de données (état, espaces, onglets, réglages) viennent de `@signet/shared/types`.

## Fichiers

Les chemins ci-dessous sont ceux d'aujourd'hui (monorepo). Les sections de ce document citent parfois un fichier par son nom seul (`store.ts`, `tabs.ts`…) : cherchez-le dans la liste. Le plan complet, avec le rôle de chaque dossier, est dans [ARCHITECTURE.md](ARCHITECTURE.md).

```
app/desktop/src/shared/types/        contrat de données et `SignetApi` (aussi publié par @signet/shared/types)
app/desktop/src/shared/ipc/          contrat IPC (contract.ts) et gardes d'arguments (validate.ts)
app/desktop/src/main/index.ts        démarrage, user agent, open-url, instance unique, enregistrement de l'IPC
app/desktop/src/main/core/           store.ts (état persistant, state.json), history.ts, atomic.ts
app/desktop/src/main/model/          règles de l'état (ouvrir, fermer, déplacer, archiver…), sans Electron
app/desktop/src/main/browser/        classe Browser (browser.ts, racine de composition) et ses parties :
                                     views, layout, broadcast, overlay, peek, undo, window, tab-actions…
app/desktop/src/main/browser/tabs.ts      création d'une WebContentsView pour un onglet et câblage de ses événements
app/desktop/src/main/browser/palette.ts   moteur de la barre de commande (requête, suggestions, URL)
app/desktop/src/main/windows/        little.ts (Little Signet), private.ts, pip.ts, settings.ts, library.ts
app/desktop/src/main/app/            menu.ts (menu et raccourcis), platform.ts, i18n.ts, appearance.ts…
app/desktop/src/main/ipc/            gestionnaires IPC, un fichier par domaine
app/desktop/src/preload/index.ts     expose window.signet (SignetApi) via contextBridge, construit d'après le contrat
app/desktop/src/renderer/src/main.tsx   routage par hash : '' → fenêtre principale, '#overlay', '#little'…
app/desktop/src/renderer/src/...     composants, rangés par domaine (voir Interface)
packages/shared/src/urls.ts          toUrl(), searchUrl() ; site.ts : siteOf()
```

## Modèle

- **Onglets** : `tabs: Record<id, TabInfo>`. Un onglet vit dans exactement un endroit : favoris (global), `pinned` ou `today` d'un espace, ou « détaché » (Peek, Little Signet — jamais persisté).
- **Favoris** : liste globale d'ids d'onglets, partagée par tous les espaces. Ils ont un `pinnedUrl`. Un favori ne peut pas faire partie d'une vue partagée.
- **Espaces** : nom, emoji, teinte, `pinned: Item[]`, `today: Item[]`, `activeTabId`. Au moins un espace existe toujours. État initial : un espace « Personnel », 🏠, dégradé teintes 250 puis 0 (violet → rouge), grain 0,5, vide.
- **Vue partagée** : `Item` de type `split` contenant 2 ou 3 `tabIds`. Les onglets d'une vue partagée n'apparaissent pas ailleurs. Quand il ne reste qu'un onglet, la vue partagée redevient un `Item` tab.
- **Onglets épinglés** : `pinnedUrl` = URL au moment de l'épinglage. « Fermer » un épinglé = le décharger (détruire la vue, `loaded=false`) et remettre `url = pinnedUrl`. Il reste dans la liste : sa ligne ne se replie donc jamais (pas d'animation `.leaving`) et le bouton ✕ s'appelle « Décharger ». **Sélection optimiste** : au clic sur un onglet ou un espace (barre latérale, rangée d'onglets), l'interface affiche aussitôt la sélection (`optimistic.ts`) ; elle tombe à la première mise à jour reçue après la réponse du main, ou 400 ms après la réponse si le main a refusé. Choisir un espace depuis le popover de la barre d'onglets du haut (processus de l'overlay) est aussi optimiste : à la réception de `space:switch`, le main envoie aussitôt `space:pending` (id) à la fenêtre principale, avant le cycle différé layout/état ; `optimistic.ts` l'adopte, il tombe à la prochaine mise à jour ou après 1 s si le main a refusé.
- **Largeur de la barre latérale, élastique** : pendant le glissement de la poignée, la largeur brute au-delà de 180–420 est affichée via `rubberband(dépassement, 160, 0.55)`. L'IPC `sidebar:width(w, live)` avec `live=true` relâche la borne à 100–500 (±80) côté main (largeur transitoire `liveSidebarWidth`, dans le layout et l'état poussé, jamais persistée). Au relâchement, `spring()` (≈0,3 s, critique, vitesse de lâcher) ramène à la valeur bornée, image par image, puis une dernière valeur non `live` est envoyée et persistée. Une nouvelle prise interrompt le ressort. Réduire les animations : borne dure, sans ressort.
- **Onglets du jour** : archivés automatiquement après `settings.archiveAfterHours` (12 par défaut) sans activation, sauf l'onglet actif. Vérification toutes les 5 minutes. Archiver = détruire la vue, retirer l'onglet, l'ajouter à `archive` (url, titre, favicon, date ; 500 max). Les archives sont cherchables dans la barre de commande.
- **Onglets fermés récemment** : pile de 20 pour ⇧⌘T (mémoire seulement, jamais écrite sur disque). Chaque entrée garde l'URL, le titre, l'icône, le nom/l'icône personnalisés, `pinnedUrl`, la place d'origine (`spaceId`, zone, `folderId`, rang) et l'historique de navigation (`TabHistory`, restauré par `navigationHistory.restore`). ⇧⌘T rouvre l'onglet **à sa place** : même espace, même zone, même dossier, même rang (bornés à la liste actuelle). Espace disparu → espace courant (un épinglé redevient onglet du jour, en tête) ; dossier disparu → fin des épinglés de l'espace. Alimentent la pile : ⌘W sur un onglet du jour, « Supprimer le dossier et ses onglets », « Supprimer l'espace » (le premier onglet de la liste en haut de la pile ; 20 au plus).
- **Archivage groupé annulable** : « Archiver » (bouton du séparateur ou action `clear-today` de la barre de commande) archive tous les onglets du jour de l'espace courant, puis le main pousse `today:archived` `{id, count}` à la barre latérale, qui affiche l'annonce « N onglets archivés · Annuler » (5 s, pause au survol/focus, ✕ ou Échap pour la fermer sans annuler). `today:undo(id)` (`undoClearToday`) remet les onglets (mêmes ids, historiques) sous les onglets ouverts depuis, retire leurs entrées de l'archive et rend l'onglet actif d'avant si l'utilisateur n'en a pas choisi un autre. Un seul niveau : un nouvel archivage remplace le précédent. L'archivage automatique (délai d'inactivité) n'est pas annulable. Quand aucune barre latérale n'est affichée (onglets en haut, mode compact, barre masquée), le main n'envoie pas l'événement à la fenêtre : il crée une petite `WebContentsView` transparente (route `#toast`, 384×72, centrée en bas de la carte web, sous l'overlay, au-dessus des pages) qui rend la même annonce (mêmes entrée/sortie), et la retire quand l'annonce a fini sa sortie (`toast:done`). Elle ne couvre que sa propre zone, suit le redimensionnement de la fenêtre (recalculée à chaque layout) et l'annulation y fonctionne.
- **Vues paresseuses** : au démarrage aucune vue n'existe. Une `WebContentsView` est créée quand l'onglet est affiché pour la première fois.
- **Déchargement automatique** (mise en veille) : toutes les minutes, un onglet masqué depuis plus longtemps que `settings.tabSleepMinutes` (Réglages › Général ; absent = 30 ; 0 = jamais) est déchargé comme un épinglé « fermé » (vue détruite, `loaded=false`, historique sauvegardé), mais **sans** toucher à son `url` : au retour il se recharge là où il était, avec son historique. Jamais déchargés : onglets affichés (panneaux d'une vue partagée compris), qui jouent ou ont joué du son récemment, dans le mini-lecteur, Peek, détachés (Little Signet) ; rien n'est déchargé pendant un téléchargement ou un glisser-déposer d'onglet.
- **Vues d'interface cachées** : la barre d'outils du mode compact n'est créée que si le mode est actif ; la barre latérale flottante est préchauffée 1,5 s après le masquage de la barre (et créée au survol sinon) ; la barre de recherche est préchauffée 4 s après l'ouverture de la fenêtre (et créée à la demande par ⌘F) ; la bulle ⌘Q est préchauffée 5 s après le lancement (et créée à la demande par ⌘Q). Les envois précoces sont mis en file par `UiView`.

### Persistance (`store.ts`)

`app.getPath('userData')/state.json`, écrit avec un délai de 500 ms après chaque changement et de façon synchrone à `before-quit`. Contenu : onglets (id, url, title, favicon, pinnedUrl, lastActive, muted), favoris, espaces, activeSpaceId, sidebarVisible, sidebarWidth (240 par défaut, borné 180–420), archive, settings, bounds de la fenêtre. Champ `version: 1`. Lecture tolérante : fichier absent ou corrompu → état initial.
Les onglets détachés (Peek, Little Signet) ne sont jamais écrits.

### Historique (`history.ts`)

`history.json`, `Map<url, {url, title, favicon, visits, lastVisit}>`, 5 000 entrées max (on retire les plus anciennes), écriture différée de 2 s. On enregistre sur `did-navigate` et `did-navigate-in-page` (cadre principal), et on met à jour le titre sur `page-title-updated`. On ignore `about:`, `data:` et `devtools:`.

## Fenêtre principale et layout

`BrowserWindow` : `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 16, y: 16 }`, 1280×820 par défaut (bounds restaurés), `minWidth 700`, `minHeight 480`, `vibrancy: 'under-window'`, `visualEffectState: 'active'`, `backgroundColor: '#00000000'`. Son `webContents` charge l'interface (route `''`) : barre latérale et fond.

Calcul du layout, refait à chaque changement d'état et sur `resize` de la fenêtre :

- `W, H = win.getContentBounds()` en largeur et hauteur ; `pad = 8`.
- Barre latérale visible : zone de contenu `x = sidebarWidth`, sinon `x = pad`. `y = pad`, `width = W - x - pad`, `height = H - 2*pad`.
- Barre latérale cachée : `win.setWindowButtonVisibility(false)`, réaffichés quand elle revient.
- Onglets affichés : ceux de l'élément qui contient `activeTabId` de l'espace actif (1 onglet, ou tous ceux de la vue partagée). Aucun si `activeTabId` est nul.
- N panneaux côte à côte, `gap = 8`, largeurs égales.
- Chaque vue : `setBorderRadius(10)`, fond blanc. Les vues qui ne sont plus affichées sont retirées de `contentView` (pas détruites). Garder un `Set` des vues attachées.
- `panes` du snapshot = rects et `focused` (onglet = `activeTabId`).
- Ordre de pile : vues des onglets, puis overlay s'il est ouvert, puis vue Peek. (Réajouter une vue déjà enfant la remet au premier plan.)

Quand l'onglet actif change : `lastActive = Date.now()`, focus clavier sur son `webContents`.

### Diffusion de l'état

Toute modification appelle `changed()`, qui regroupe les envois (`setImmediate`) : recalcule le layout, envoie `state` (Snapshot) au `webContents` principal, envoie `little:state` à chaque fenêtre Little Signet et planifie la sauvegarde.

## Onglets (`tabs.ts`)

Créer une vue : `new WebContentsView({ webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false } })`.
Événements à câbler sur son `webContents` et à répercuter sur `TabInfo` puis `changed()` :

- `page-title-updated`, `page-favicon-updated` (premier favicon), `did-start-loading` / `did-stop-loading`
- `did-navigate`, `did-navigate-in-page` (cadre principal) → `url`, `canGoBack/canGoForward` via `webContents.navigationHistory`, historique
- `audio-state-changed` → `audible`
- `setWindowOpenHandler(({ url, disposition, features }))` :
  - `disposition === 'new-window'` et `features` non vide → popup (souvent une connexion OAuth) : `{ action: 'allow' }` avec une fenêtre 520×720
  - `disposition === 'new-window'` sans `features` (⇧ clic) → Peek, `deny`
  - `background-tab` (⌘ clic) → nouvel onglet du jour juste après l'élément courant, sans l'activer, `deny`
  - sinon → nouvel onglet du jour activé, `deny`
- `will-navigate` : si `settings.peekExternalLinks`, que l'onglet a un `pinnedUrl`, que c'est le cadre principal et que `siteOf(nouvelle url) !== siteOf(url actuelle)` → `preventDefault()` et ouvrir dans Peek
- `before-input-event` : Échap dans la vue Peek → fermer Peek
- `context-menu` → menu natif en français :
  - sur un lien : Ouvrir dans un nouvel onglet, Ouvrir dans Peek, Ouvrir en vue partagée, Copier le lien
  - sur une image : Ouvrir l'image dans un nouvel onglet, Copier l'image (`copyImageAt`), Enregistrer l'image (`downloadURL`)
  - texte sélectionné : Copier, Rechercher « extrait de 30 caractères » (nouvel onglet)
  - champ éditable : Annuler, Rétablir, Couper, Copier, Coller, Tout sélectionner (rôles)
  - sinon : Précédent, Suivant, Recharger
  - toujours en dernier : séparateur + Inspecter (`inspectElement`)

## URL (`urls.ts`)

- `toUrl(input)` : schéma présent (`https://`, `about:`, `file:`, `data:`, `view-source:`…) → tel quel ; `localhost[:port]` ou IPv4 → `http://` ; pas d'espace et ressemble à un domaine (`x.yz`, port et chemin optionnels) → `https://` ; sinon `null`.
- `searchUrl(q)` : `https://www.google.com/search?q=…&hl=<langue>`, `hl` tiré de la première langue des sites (`setSearchLanguage`, appelé par `applyLanguages`) : la langue de base (`en-US` → `en`), sauf `zh-…` et `pt-…` qui gardent leur région. Sans `hl`, Google choisit la langue d'après l'adresse IP (en Corée : en coréen) et ignore `Accept-Language`.
- `siteOf(url)` : nom d'hôte sans `www.`, réduit au domaine enregistrable approximatif (2 derniers labels, 3 si l'avant-dernier fait ≤ 3 lettres et le dernier 2 lettres, comme `co.uk`).

## Barre de commande (`palette.ts` + overlay)

L'overlay est une `WebContentsView` unique par fenêtre principale, créée au démarrage, fond transparent (`setBackgroundColor('#00000000')`), qui charge la route `#overlay`. Ouvrir = l'ajouter en haut de la pile, couvrant toute la fenêtre, envoyer `overlay:show` et lui donner le focus clavier. Fermer = la retirer, envoyer `overlay:hide` et rendre le focus à l'onglet actif. Palette, panneau du site et popover jouent une sortie de 120 ms (le main garde la vue posée, `OVERLAY_EXIT_MS`) ; **sauf** pour un clic hors du panneau (`closeOverlay(true)`) : la vue est retirée aussitôt (l'utilisateur agit déjà ailleurs, elle ne doit pas avaler son prochain clic). Un clic sur l'overlay pendant une sortie (Échap, action) la termine aussi immédiatement. Entrées et sorties des surfaces de verre (voile, palette, panneau du site, popover, panneaux `.ex` de la barre latérale) sont des **transitions CSS** (`@starting-style` pour l'entrée, classe `closing` pour la sortie ; échelle 0,96 + opacité, fondu seul en mouvement réduit) : fermer en pleine entrée repart de la valeur courante, rouvrir en pleine sortie reprend de là (sauf la palette, remontée à neuf).

Modes :

- `new-tab` (⌘T, bouton « Nouvel onglet ») : champ vide.
- `edit-url` (⌘L, clic sur l'URL) : champ prérempli avec l'URL de l'onglet actif, texte sélectionné ; valider navigue dans l'onglet actif (s'il n'y en a pas, nouvel onglet).
- `split` (⌃⇧=, action « Vue partagée ») : le résultat choisi rejoint une vue partagée avec l'onglet actif. Un onglet existant choisi est déplacé dans la vue partagée. Max 3 panneaux. Si l'onglet actif est un favori ou qu'il n'y en a pas : le résultat s'ouvre simplement dans un nouvel onglet.

`paletteQuery(q, mode)` (synchrone côté main, rapide), max ~12 résultats, dans cet ordre :

1. q vide : en mode `new-tab`, les 6 onglets ouverts les plus récents de l'espace actif, puis 4 actions utiles.
2. Si `toUrl(q)` → résultat `url` ; sinon résultat `search` « Rechercher « q » sur <moteur> » (moteur choisi dans Réglages › Général ou à l'introduction, Google par défaut ; les suggestions viennent du même moteur).
3. Onglets ouverts (tous espaces) dont le titre ou l'URL contient q → `tab`, sous-titre « Aller à l'onglet · nom de l'espace ».
4. Actions dont le libellé contient q (insensible à la casse et aux accents).
5. Historique : correspondance titre/URL, score = visites + bonus de récence ; exclure les URL déjà ouvertes.
6. Archives correspondantes.

`paletteSuggest(q)` : `net.fetch('https://duckduckgo.com/ac/?type=list&q=…')` → 4 résultats `suggestion` max ; erreur réseau → `[]`. L'interface l'appelle après 150 ms sans frappe et ajoute les suggestions après le premier résultat ; pendant la frappe, les suggestions précédentes qui correspondent encore au début de la saisie restent affichées jusqu'à l'arrivée des nouvelles.

Actions (id → libellé, raccourci affiché) :

- `new-space` Nouvel espace (ouvre le formulaire dans la barre latérale via `space:edit` avec `spaceId: null`)
- `split` Vue partagée (⌃⇧=) → rouvre la barre en mode `split`
- `unsplit` Quitter la vue partagée
- `toggle-sidebar` Afficher / masquer la barre latérale (⌘S)
- `pin` Épingler / Désépingler l'onglet (⌥⌘P ; ⌘D ajoute la page aux signets, voir v16)
- `favorite` Ajouter aux favoris / Retirer des favoris
- `copy-url` Copier l'URL (⇧⌘C)
- `little` Nouvelle fenêtre Little Signet (⌥⌘N)
- `clear-today` Archiver les onglets du jour
- `close-tab` Fermer l'onglet (⌘W)
- `reopen` Rouvrir l'onglet fermé (⇧⌘T)
- `reload` Recharger (⌘R), `devtools` Outils de développement (⌥⌘I)
- `toggle-peek-links` Ouvrir les liens externes des épinglés dans Peek : activé/désactivé
- `default-browser` Définir comme navigateur par défaut (`app.setAsDefaultProtocolClient('http')` et `'https'` ; ne marche que dans l'app empaquetée, l'indiquer dans le sous-titre)
- une action `space:<id>` « Aller à l'espace <nom> » par espace

`paletteExecute(result, mode)` ferme l'overlay puis : `url`/`search`/`suggestion`/`history`/`archived` → ouvrir l'URL selon le mode ; `tab` → activer (changer d'espace si besoin) ou, en mode `split`, déplacer dans la vue partagée ; `action` → exécuter.

## Peek

Un seul Peek à la fois dans la fenêtre principale. Ouvrir : créer un onglet détaché avec sa vue, ouvrir l'overlay en mode `peek` (fond assombri + cadre + boutons), puis ajouter la vue web par-dessus.
Carte : `width = min(W - 160, 1100)`, `height = H - 96`, centrée horizontalement, `y = 64`. Vue web : `setBorderRadius(12)`. Le `card` du payload sert à l'interface pour dessiner l'ombre et placer les boutons au-dessus à droite (Fermer ✕, Ouvrir en onglet ⤢).

- Clic sur le fond assombri, Échap (dans l'overlay ou dans la page) → fermer : détruire la vue et l'onglet.
- `expandPeek` → retirer l'overlay, insérer l'onglet en tête des onglets du jour de l'espace actif, l'activer.
- Le titre du payload se met à jour : renvoyer `overlay:show` quand le titre change.
- Ouvrir la barre de commande pendant un Peek ferme d'abord le Peek.

## Little Signet (`little.ts`)

Petite fenêtre pour ouvrir un lien vite fait (⌥⌘N, liens externes reçus par `open-url`, second lancement avec une URL).
`BrowserWindow` 760×820, `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 14, y: 14 }`, `vibrancy: 'under-window'`, en cascade de 24 px par rapport à la précédente. Charge la route `#little` (barre de 44 px : précédent, suivant, titre et domaine, bouton « Copier l'URL », bouton « Ouvrir dans Signet »). Onglet détaché dont la vue est posée en `{ x: 6, y: 44, width: W - 12, height: H - 50 }`, `setBorderRadius(10)`. ⌥⌘N sans URL → la fenêtre s'ouvre sur la barre de commande… trop compliqué en v1 : ouvrir `https://www.google.com`.

- `littleOpenInMain` (depuis le `webContents` de la barre, retrouver la fenêtre via `BrowserWindow.fromWebContents(event.sender)`) : retirer la vue de la petite fenêtre, insérer l'onglet en tête des onglets du jour de l'espace actif, l'activer, montrer et focaliser la fenêtre principale, fermer la petite fenêtre sans détruire l'onglet.
- Fermer la fenêtre détruit l'onglet.
- Les raccourcis du menu (⌘W, ⌘R, ⌘[, ⌘], ⌘L…) agissent sur la Little Signet si c'est la fenêtre au premier plan.

## Fenêtres privées (`private.ts`, route `#private`)

Ouverture : ⇧⌘N (menu Fichier), action `private` de la barre de commande, « Ouvrir dans une fenêtre privée » du menu contextuel d'un lien. Plusieurs fenêtres possibles, en cascade de 24 px.

- **Session** : une par fenêtre, `session.fromPartition('private-<uuid>')` (sans `persist:`, donc en mémoire). Deux fenêtres privées ne partagent rien. Configurée par `configurePrivateSession` : langues, bloqueur, téléchargements, pages `signet:`, glisser pour naviguer ; **pas d'extensions** ; autorisations dans une table en mémoire propre à la session (jamais dans `sitePermissions`). À la fermeture : `clearStorageData`, `clearCache`, `clearAuthCache`, retrait du bloqueur.
- **Onglets** : détachés (`store.detached`, jamais persistés ni visibles dans la barre latérale), session héritée (`inheritSession`). Rien dans l'historique (`history.record`/`update` ignorés), pas de ⇧⌘T, jamais déchargés. Fermer le dernier onglet ferme la fenêtre ; fermer la fenêtre oublie tous ses onglets.
- **Liens** : `window.open`, `target=_blank`, « Ouvrir dans un nouvel onglet », recherche de la sélection, image → nouvel onglet juste après, dans la même fenêtre. Les popups avec `features` (OAuth) restent de vraies fenêtres (même session). Peek, vue partagée, Boost et mini-lecteur absents du menu contextuel.
- **Téléchargements** : le fichier va dans Téléchargements ; l'entrée (`private: true`) n'est jamais écrite dans state.json et quitte la liste à la fermeture de la fenêtre (une fois fini).
- **Disposition** : celle de la fenêtre principale (`settings.tabLayout`), suivie en direct (⌃⇧S, largeur de la barre) ; `PrivateState.sidebarWidth` vaut la largeur de la barre, ou null pour les onglets en haut.
  - Onglets en haut : rangée de 40 px (feux tricolores à `{16, 13}`, pastilles `tchip day`, bouton +, badge « Privée »), carte en `{ x: 8, y: 44, width: W - 16, height: H - 52 }`.
  - Barre latérale (largeur `sidebarWidth`) : en-tête de 44 px (feux tricolores à `{16, 16}`, badge à droite), lignes `row` des onglets (onglet affiché sur `--active`), ligne « Nouvel onglet ». Pas d'espaces, de favoris, d'épinglés ni de dossiers. Carte en `{ x: sidebarWidth, y: 8, width: W - sidebarWidth - 8, height: H - 16 }`.
- **Interface** : la carte porte une barre de 40 px avec Précédent, Suivant, Recharger et un champ d'adresse (domaine / titre au repos, adresse complète à l'édition, Entrée → `privateNavigate`, Échap annule). Vue web posée sous cette barre, sur le reste de la carte. Onglet vide : pas de vue, écran d'accueil qui explique ce qui est gardé ou non, champ d'adresse focalisé. Teinte fixe `--h: 285` et fond plus teinté. La barre demande son état au démarrage (`private:get`), puis reçoit `private:state`.
- **Raccourcis** dans une fenêtre privée au premier plan : ⌘T nouvel onglet, ⌘W fermer l'onglet, ⌘L champ d'adresse, ⌃Tab / ⌃⇧Tab / ⇧⌘] / ⇧⌘[ onglet voisin, ⌘1…⌘9 ; ⌘R, ⌘[, ⌘], zoom et outils de développement visent l'onglet affiché. Les autres commandes ramènent la fenêtre principale. ⌘F n'agit pas.
- Canaux : `private:open`, `private:get`, `private:new-tab`, `private:activate`, `private:close`, `private:navigate` ; événements `private:state`, `private:focus-address`. Vérification : `SIGNET_PRIVATECHECK=<dossier>` (profil isolé).

## Menu et raccourcis (`menu.ts`)

Les accélérateurs du menu d'application marchent quel que soit le panneau qui a le focus. Chaque commande cible `BrowserWindow.getFocusedWindow()` : fenêtre principale ou Little Signet.

- **Signet** : À propos, Masquer, Masquer les autres, Quitter (⌘Q)
- **Fichier** : Nouvel onglet ⌘T, Nouvelle fenêtre Little Signet ⌥⌘N, Nouvelle fenêtre privée ⇧⌘N, Rouvrir l'onglet fermé ⇧⌘T, Fermer l'onglet ⌘W (dans Little Signet : fermer la fenêtre), Fermer la fenêtre ⇧⌘W
- **Édition** : rôles undo, redo, cut, copy, paste, pasteAndMatchStyle, selectAll
- **Affichage** : Afficher/masquer la barre latérale ⌘S, Recharger ⌘R, Forcer le rechargement ⇧⌘R, Zoom avant ⌘+ (et ⌘=), Zoom arrière ⌘-, Taille réelle ⌘0 (sur l'onglet actif, `setZoomLevel`), Plein écran (rôle), Outils de développement ⌥⌘I
- **Navigation** : Précédent ⌘[, Suivant ⌘], Ouvrir l'adresse ⌘L, Copier l'URL ⇧⌘C, Épingler/Désépingler ⌥⌘P, Signet ⌘D, Vue partagée ⌃⇧=, Onglet suivant ⌃Tab ou ⇧⌘], Onglet précédent ⌃⇧Tab ou ⇧⌘[ (parcourt favoris, épinglés puis onglets du jour de l'espace actif), Onglet 1 à 8 ⌘1…⌘8 et Dernier onglet ⌘9 (`gotoTabNumber` : ordre de la barre latérale de haut en bas — favoris, épinglés, jour ; les onglets d'un dossier fermé ne comptent pas, ni aucun dossier avec les onglets en haut), Espace suivant ⌥⌘→, Espace précédent ⌥⌘←, Espace 1 à 9 ⌃1…⌃9
- **Fenêtre** : rôle windowMenu

## Démarrage (`index.ts`)

- `app.setName('Signet')`.
- User agent : retirer ` Electron/x` et ` signet/x` (insensible à la casse) de `app.userAgentFallback`, et réduire la version comme Chrome et CEF (`Chrome/156.0.0.0` au lieu de `Chrome/156.0.8078.48`) : identity.ts `reducedUserAgent`.
- `requestSingleInstanceLock()` ; `second-instance` : si une URL http(s) est dans argv → Little Signet, sinon focaliser la fenêtre principale.
- `open-url` (peut arriver avant `ready` : mettre en file d'attente) → Little Signet.
- Permissions (`session.defaultSession.setPermissionRequestHandler`) : autoriser sans demander `clipboard-sanitized-write`, `fullscreen`, `pointerLock` ; pour le reste, `dialog.showMessageBox` en français (« example.com demande l'accès à la caméra et au micro / à votre position / aux notifications / au presse-papiers… », Autoriser / Refuser), décision mémorisée pour la session par `origine|permission`. Refuser par défaut tout ce qui n'est pas listé dans les libellés. Une demande venue d'un onglet non affiché attend qu'il le soit avant d'ouvrir la boîte ; fermé entre-temps, elle est refusée sans être mémorisée.
- Échec de chargement d'un onglet (`did-fail-load` du cadre principal, hors `ERR_ABORTED`) : la page d'erreur du moteur est remplacée par une page claire (titre, hôte, cause en langage clair selon le code, code brut en repli, bouton « Réessayer » = `location.reload()`). L'onglet garde l'adresse d'origine, l'historique n'enregistre rien. Plantage du processus (`render-process-gone`, hors sortie normale) : page « Cette page a planté » avec « Recharger » ; ⌘R / bouton Recharger rouvrent l'adresse d'origine (`reloadContents`, tabs.ts). Textes : `app/desktop/src/main/web/errorpage.ts`.
- `activate` : recréer la fenêtre principale si elle n'existe pas. `window-all-closed` : ne pas quitter sur macOS.
- Tous les `ipcMain.handle` du contrat `SignetApi`. Canaux : le nom de la méthode en kebab-case préfixé du domaine, par exemple `tab:activate`, `tab:close`, `tab:navigate`, `tab:back`, `tab:forward`, `tab:reload`, `tab:toggle-mute`, `tab:reset-pinned`, `tab:menu`, `item:move`, `space:switch`, `space:create`, `space:update`, `space:menu`, `sidebar:toggle`, `sidebar:width`, `today:clear`, `pane:focus`, `split:remove`, `palette:open`, `overlay:close`, `palette:query`, `palette:suggest`, `palette:execute`, `peek:expand`, `little:open-in-main`, `little:copy-url`, `state:get`. Événements poussés : `state`, `space:edit`, `overlay:show`, `overlay:hide`, `little:state`. **Le preload et le main doivent utiliser exactement ces noms.**
- Chemins : preload `join(__dirname, '../preload/index.js')` ; interface en dev `process.env.ELECTRON_RENDERER_URL + '#route'`, en prod `loadFile(join(__dirname, '../renderer/index.html'), { hash })`.

### Menus contextuels de la barre latérale (natifs, construits dans le main)

- Onglet (`tab:menu`) : Épingler / Désépingler, Ajouter aux favoris / Retirer des favoris, Vue partagée avec l'onglet actif (si ce n'est pas lui), Retirer de la vue partagée (s'il est dans une), Dupliquer, Copier l'URL, Déplacer vers l'espace ▸ (sous-menu des autres espaces), séparateur, Fermer.
- Espace (`space:menu`) : Modifier l'espace… (envoie `space:edit`), Supprimer l'espace… (confirmation, désactivé s'il n'en reste qu'un ; le détail rappelle que les onglets se rouvrent avec ⇧⌘T et, pour un profil isolé, que cookies et connexions sont effacés définitivement).

### Glisser-déposer (`item:move`)

Retirer la source de là où elle est (favoris, épinglés ou jour de n'importe quel espace) puis l'insérer à `target.index` (borné) dans la zone visée. Vers `favorites` : seulement un onglet, `pinnedUrl = url`. Vers `pinned` : `pinnedUrl = url` (pour chaque onglet d'une vue partagée). Vers `today` : `pinnedUrl` supprimé. Si l'onglet déplacé était actif dans l'espace d'origine et qu'on change d'espace, l'espace d'origine choisit un autre onglet actif. Attention à l'index quand on déplace dans la même liste vers le bas.

### Choix de l'onglet actif après une fermeture

L'élément suivant dans la même liste, sinon le précédent, sinon le premier épinglé, sinon `null` (fond vide).

## Interface (React)

Les feuilles de style sont dans `app/desktop/src/renderer/src/styles/` (`index.css` les importe dans l'ordre de la cascade ; c'était une seule feuille `styles.css` à l'origine). Police système (`-apple-system`), 13 px. Mode clair et sombre (`prefers-color-scheme`). Couleur d'espace : variable CSS `--h` = teinte de l'espace ; fond de la barre latérale = dégradé `hsl(var(--h) 70% 86% / .78)` → `hsl(calc(var(--h) + 35) 65% 80% / .78)` en clair, versions sombres (`20%` de luminosité) en sombre, transition de 300 ms au changement d'espace. Tout l'arrière-plan de la fenêtre principale prend cette couleur (les vues web sont posées dessus avec leurs coins arrondis). Pas de bordures dures : survols en `rgb(255 255 255 / .35)` (clair) ou `/.08` (sombre), éléments actifs en blanc à 70 % avec une légère ombre.

### Fenêtre principale (route `''`)

Barre latérale à gauche, largeur `sidebarWidth`, de haut en bas :

1. Barre du haut, 44 px, `-webkit-app-region: drag`, marge gauche 78 px pour les feux tricolores : boutons (en `no-drag`) Barre latérale, Précédent, Suivant, Recharger.
2. Pilule d'URL : domaine de l'onglet actif (sans `www.`), cadenas si https ; clic → `openPalette('edit-url')`.
3. Favoris : grille de 4 colonnes de tuiles carrées avec le favicon, tuile active surlignée ; masquée s'il n'y en a pas, mais zone de dépôt visible pendant un glisser.
4. En-tête d'espace : nom seul (l'emoji est dans la barre du bas) ; clic droit → `showSpaceMenu`.
5. Épinglés. Un épinglé dont l'URL a quitté son `pinnedUrl` affiche une petite barre « / » cliquable qui appelle `resetPinned`.
6. Séparateur avec bouton « Archiver » au survol (`clearToday`, annulable par l'annonce « N onglets archivés · Annuler »).
7. Ligne « + Nouvel onglet » → `openPalette('new-tab')`.
8. Onglets du jour (défilement si besoin).
9. Barre du bas : un bouton par espace (son emoji ; l'actif est plus opaque), « + » → formulaire nouvel espace. Les boutons d'espace sont aussi des zones de dépôt (dépôt = déplacer en tête des onglets du jour de cet espace).

Ligne d'onglet : 32 px, favicon 16 px (icône globe par défaut, pulsation pendant le chargement), icône haut-parleur entre le favicon et le titre si `audible` ou `muted` (clic → `toggleMute` ; à gauche, la croix qui apparaît au survol ne la déplace pas), titre tronqué, bouton ✕ au survol (`closeTab`, sans place réservée quand il est caché). Clic → `activateTab`. Clic droit → `showTabMenu`. Clic milieu → fermer.

Chargement des favicons (partout dans l'interface, composant `Favicon`) : l'interface charge l'adresse du site ; en cas d'échec (en-têtes du site, connexion requise, réseau absent), elle demande au main (`favicon:fallback`, app/desktop/src/main/web/favicons.ts), qui la récupère avec les cookies de la session (par défaut, puis espaces isolés) et renvoie une data URL. Réussites gardées en mémoire, échecs retentés après une minute (avant : un échec affichait le globe jusqu'au redémarrage).
Ligne de vue partagée : un conteneur arrondi avec un segment par onglet séparé par un trait fin ; le segment cliqué → `focusPane` (et active l'espace) ; le segment actif est surligné.

Glisser-déposer HTML5 : lignes et tuiles `draggable`, source en `dataTransfer` (JSON de `DragSource`) ; au survol d'une ligne, insertion avant ou après selon la moitié verticale (trait indicateur de 2 px) ; zone vide en fin de liste = ajout à la fin ; dépôt → `moveItem`.

Poignée de redimensionnement : bande de 6 px au bord droit de la barre latérale (`cursor: col-resize`), `setSidebarWidth` pendant le glisser.

Formulaire d'espace : petit panneau dans la barre latérale (jamais au-dessus des vues web, qui le masqueraient) : champ nom, champ emoji, 10 pastilles de teinte (0, 25, 45, 90, 150, 190, 215, 250, 285, 330), Annuler / Créer ou Enregistrer. Ouvert par le « + », ou par `onEditSpace`.

Zone de contenu (à droite de la barre latérale) : rien quand des panneaux sont affichés (les vues web la couvrent), sauf un anneau de focus de 2 px (couleur de l'espace, plus saturée) autour du panneau actif quand il y en a plus d'un, dessiné à `rect` agrandi de 3 px avec un rayon de 13 px. Quand `panes` est vide : état vide centré, « Aucun onglet ouvert » et un bouton « Nouvel onglet ⌘T ».

Barre latérale masquée : la zone de contenu prend toute la largeur ; pas de barre latérale flottante en v1.

### Overlay (route `#overlay`)

`html, body` transparents. Écoute `onOverlayShow` / `onOverlayHide` ; vide son état à la fermeture.

- **Barre de commande** : voile `rgb(0 0 0 / .12)` sur toute la fenêtre (clic → `closeOverlay`), panneau de 640 px de large centré à 18 % du haut, rayon 14 px, fond clair translucide (`backdrop-filter: blur(30px)`) avec un liseré de la couleur de l'espace ; champ de 52 px avec autofocus (texte présélectionné en mode `edit-url`), libellé de mode discret (« Vue partagée » en mode `split`). Liste : icône selon le type (favicon pour onglets, historique et archives ; loupe pour recherche et suggestions ; globe pour URL ; icône d'action), titre, sous-titre gris, raccourci à droite pour les actions. ↑ ↓ pour naviguer, Entrée pour exécuter, Échap pour fermer, survol souris = sélection. Requête à chaque frappe ; suggestions après 150 ms. Animation d'entrée : fondu + échelle 0,98 → 1 en 120 ms.
- **Peek** : voile `rgb(0 0 0 / .35)` (clic → `closeOverlay`), ombre portée sous `card` (la vue web se pose par-dessus), deux boutons ronds de 28 px au-dessus de la carte à droite (`card.y - 36`) : ⤢ « Ouvrir en onglet » (`expandPeek`) et ✕ (`closeOverlay`), et le titre de la page en blanc à gauche sur la même ligne. Échap → `closeOverlay`.

### Little Signet (route `#little`)

Fond teinté par `hue` (même dégradé que la barre latérale). Barre de 44 px en `drag`, marge gauche 76 px : Précédent, Suivant (`goBack()`/`goForward()` sans argument : le main cible l'onglet de la fenêtre émettrice), favicon + titre + domaine au centre, boutons « Copier l'URL » et « Ouvrir dans Signet » à droite.

## v2

Nouveaux éléments du contrat : `Settings.adblock`, `DownloadInfo`, `FindState`, `Snapshot.downloads`, `Snapshot.sidebarFloating` et des méthodes de `SignetApi` (sections « Barre latérale flottante », « Recherche dans la page », « Téléchargements »).
Nouveaux canaux : `sidebar:hover`, `find:query`, `find:close`, `download:open`, `download:reveal`, `download:cancel`, `downloads:clear` ; événement poussé `find:state`.
Nouvelles routes de l'interface : `#find` et `#sidebar`.

`store.ts` : migration douce. Un `state.json` de la v1 n'a ni `settings.adblock` (→ `true`) ni `downloads` (→ `[]`) : compléter avec les valeurs par défaut au chargement, sans changer `version`. Les téléchargements sont persistés (30 max) ; au chargement, ceux restés `progressing` ou `paused` passent à `interrupted`.

### Ordre de pile des vues

Vues des onglets → barre de recherche → barre latérale flottante → overlay → vue Peek.

### Recherche dans la page

- Une `WebContentsView` « find » par fenêtre principale, créée au démarrage comme l'overlay, fond transparent, route `#find`, 360×48.
- Position : en haut à droite du panneau actif, `x = rect.x + rect.width - 360 - 12`, `y = rect.y + 12`. Recalculée à chaque layout.
- Menu **Édition** (après les rôles) : Rechercher… ⌘F, Occurrence suivante ⌘G, Occurrence précédente ⇧⌘G. Sans effet si aucun onglet n'est actif ou si la fenêtre au premier plan est une Little Signet.
- ⌘F : ouvre la barre sur l'onglet actif (incrémente `session`, garde la dernière requête de cet onglet si elle existe, sinon vide) et lui donne le focus clavier. Si elle est déjà ouverte, lui redonne le focus et sélectionne le texte (nouvel envoi de `find:state` avec la même `session`).
- `find(query, opts)` : requête vide → `stopFindInPage('clearSelection')`, `active = matches = 0`. Sinon `findInPage(query, { forward, findNext })` sur l'onglet actif. L'événement `found-in-page` (`result.finalUpdate`) met à jour `active` / `matches` → `find:state`.
- ⌘G / ⇧⌘G : si la barre est ouverte et la requête non vide, occurrence suivante / précédente ; sinon équivaut à ⌘F.
- `closeFind()` (Échap, bouton ✕) : `stopFindInPage('clearSelection')`, retirer la vue, rendre le focus à l'onglet.
- La barre se ferme toute seule quand l'onglet actif change, quand l'espace change ou quand l'onglet navigue vers une autre page (`did-navigate`, pas `did-navigate-in-page`).
- Interface `#find` : `html, body` transparents ; une pilule arrondie (rayon 12, fond translucide flouté, ombre douce, liseré de la teinte) qui remplit la vue : icône loupe, champ (autofocus, texte sélectionné à chaque nouvel état reçu avec focus), compteur « 3 sur 12 » ou « Aucun résultat » (gris, rouge doux si 0 avec une requête), boutons ↑ ↓ et ✕. Frappe → `find(q, { forward: true, findNext: false })` ; Entrée → `findNext: true, forward: true` ; ⇧Entrée → `forward: false, findNext: true` ; Échap → `closeFind()`. Nouvelle `session` → réinitialiser le champ avec `query`.

### Téléchargements

- `session.defaultSession.on('will-download')` : enregistrer directement dans le dossier des téléchargements sans boîte de dialogue, nom unique (`nom (1).ext`, `nom (2).ext`…). Créer un `DownloadInfo` en tête de liste (30 max) et le mettre à jour sur `updated` (octets, `paused` si `item.isPaused()`, `interrupted`) et `done` (`completed`, `cancelled`, `interrupted`). Pour éviter d'inonder l'interface, limiter les `changed()` dus à `updated` à un tous les 250 ms.
- Garder les `DownloadItem` vivants dans une `Map<id, DownloadItem>` pour `cancelDownload`.
- Dossier des téléchargements (Réglages › Général) : `settings.downloadDir` (chemin absolu, choisi par « Changer… » dans une boîte « Choisir un dossier » du système ; « Par défaut » l'efface). Absent, ou dossier disparu (disque débranché, dossier supprimé) : `app.getPath('downloads')`.
- « Demander où enregistrer chaque fichier » (`settings.askDownloadPath`, absent = non) : pas de `setSavePath`, `item.setSaveDialogOptions({ defaultPath })` sur le chemin unique du dossier ; Electron ouvre sa boîte d'enregistrement. L'entrée prend le chemin choisi dès qu'il est connu ; boîte annulée → l'entrée disparaît de la liste.
- Dock : `win.setProgressBar(fraction)` de l'ensemble des téléchargements en cours (−1 quand il n'y en a plus, `2` = indéterminé si une taille est inconnue). À la fin d'un téléchargement réussi : `app.dock?.downloadFinished(path)`.
- `openDownload` → `shell.openPath` ; `revealDownload` → `shell.showItemInFolder` ; si le fichier n'existe plus, ne rien faire. `clearDownloads` retire tout sauf les téléchargements en cours.
- Un téléchargement déclenché depuis un onglet qui n'a jamais affiché de page (ouvert juste pour le fichier, URL `about:blank` ou titre vide) : fermer cet onglet du jour automatiquement.
- Interface : dans l'en-tête de la barre latérale, à gauche du bouton « Nouveau dossier » (qui ne bouge donc pas quand il apparaît), un bouton téléchargements (icône `Download`), visible seulement s'il y a au moins un téléchargement. Pendant un téléchargement, un anneau de progression autour de l'icône. Un point de la teinte de l'espace signale un téléchargement terminé que l'utilisateur n'a pas encore vu (état local à l'interface : id le plus récent vu à l'ouverture du panneau).
- Clic → panneau dans la barre latérale (même principe que le formulaire d'espace : au-dessus de la barre du bas, jamais au-dessus des vues web) : titre « Téléchargements », bouton « Effacer », liste. Chaque ligne : icône selon l'extension (`FileText`, `FileImage`, `FileArchive`, `FileVideo`, `FileAudio`, `File`), nom tronqué au milieu, sous-titre (« 12,4 Mo sur 80 Mo », « Terminé · 80 Mo », « Annulé », « Interrompu »), barre de progression fine pendant le téléchargement. Clic sur une ligne terminée → `openDownload` ; bouton loupe → `revealDownload` ; bouton ✕ pendant le téléchargement → `cancelDownload`. Tailles en unités françaises (o, Ko, Mo, Go, virgule décimale).
- Le panneau se ferme au clic en dehors de lui ou sur Échap.

### Bloqueur de pubs

- `@ghostery/adblocker-electron` (déjà installé, dans `dependencies`). Moteur `ElectronBlocker.fromPrebuiltAdsAndTracking(net.fetch, { path: <userData>/adblock.bin, read: fs.promises.readFile, write: fs.promises.writeFile })` pour le cache disque.
- Chargé au démarrage en arrière-plan (ne pas retarder l'ouverture de la fenêtre). `settings.adblock` vrai → `enableBlockingInSession(session.defaultSession)`. Une erreur (hors ligne au premier lancement) ne doit rien casser : la consigner, réessayer au prochain basculement.
- Le blocage cosmétique s'appuie sur un preload injecté par la bibliothèque. Vérifier dans son code s'il est compatible avec `sandbox: true` sur les vues des onglets. S'il ne l'est pas, ne pas désactiver le sandbox : garder seulement le blocage réseau et le signaler.
- Action de la barre de commande `toggle-adblock` : « Bloqueur de pubs : activé / désactivé » → `enableBlockingInSession` / `disableBlockingInSession`, persisté.
- Mettre à jour le moteur si le cache a plus de 7 jours (recharger depuis le réseau en arrière-plan).

### Barre latérale flottante

- Quand `sidebarVisible` est faux, la bande de 8 px du bord gauche de la fenêtre principale (route `''`) détecte le survol (`mouseenter`) → `setSidebarHover(true)`.
- Le main affiche alors une `WebContentsView` « sidebar » (créée au démarrage, route `#sidebar`, même preload) en `{ x: 6, y: 6, width: sidebarWidth, height: H - 12 }`, `setBorderRadius(12)`, fond transparent, et met `sidebarFloating = true`. Il lui envoie `state` comme à la fenêtre principale. Les feux tricolores restent cachés.
- Interface `#sidebar` : le même composant `Sidebar`, sur un fond plein (dégradé de l'espace, presque opaque car une vue web n'a pas de vibrancy) avec un liseré clair de 1 px ; `mouseleave` du document → `setSidebarHover(false)`. Ne pas réserver la place des feux tricolores dans la barre du haut quand la route est `#sidebar`.
- Le main cache la barre flottante dans trois cas seulement : `setSidebarHover(false)` (après 250 ms, annulé si `true` revient entre-temps), ⌘S (la barre redevient visible normalement), ouverture de l'overlay. Activer un onglet depuis la barre flottante ne la cache pas : c'est la sortie de la souris qui le fait.
- L'interface n'appelle pas `setSidebarHover(false)` tant que le formulaire d'espace ou le panneau des téléchargements est ouvert dans la barre flottante.
- Le fond (verre presque opaque) est porté par l'élément qui glisse (`.float-sidebar`, de même `.float-toolbar` pour la barre d'outils du mode compact), jamais par la page de la vue, qui reste transparente : le panneau arrive d'un bloc avec son contenu, au lieu d'apparaître vide à sa place avant que le contenu ne glisse dedans. Vérification image par image : `SIGNET_FLOATCHECK=<dossier>` (avec `SIGNET_SEED=1`, profil isolé).
- Pendant un glisser-déposer qui part de la barre flottante, `mouseleave` ne doit pas la fermer (ignorer `mouseleave` pendant un `drag`).

## v3

Aucune fonctionnalité d'IA.

Nouveaux éléments du contrat : `Item` de type `folder`, `DropTarget.folderId`, `DragSource` de type `folder`, `Settings.appIcon`, `APP_ICONS`, `Boost`, `BoostEditorState`, `image` sur les résultats `action`, et des méthodes de `SignetApi` (sections « Dossiers d'épinglés » et « Boosts »).
Nouveaux canaux : `folder:create`, `folder:rename`, `folder:toggle`, `folder:menu`, `boost:save`, `boost:zap`, `boost:stop-zap`, `boost:unzap`, `boost:delete` ; événements poussés `folder:rename` (main → interface, demande de renommage) et `boost:state`. Le canal `folder:rename` sert dans les deux sens : `ipcMain.handle('folder:rename')` pour `renameFolder`, `webContents.send('folder:rename')` pour `onRenameFolder`.
Nouvelle route : `#boost`.

Migration du store (sans changer `version`) : `settings.appIcon` → `'aurore'`, `boosts` → `{}`.

### Dossiers d'épinglés

- Un dossier n'existe que dans `pinned`, à la racine ou dans un autre dossier (imbrication libre, profondeur d'affichage limitée à 6). Ni les favoris ni les onglets du jour ne contiennent de dossier. Tout onglet qui entre dans un dossier reçoit un `pinnedUrl`.
- Tout le code qui parcourt les listes (`locate`, `extract`, `moveItem`, `repick`/`pickNext`, `displayedTabIds`, `cycleTab`, recherche de la barre de commande, suppression d'espace, `itemTabIds`) doit descendre récursivement dans les dossiers. Écrire une fois des utilitaires d'arbre (parcours, recherche du parent et de l'index, retrait, insertion) et s'en servir partout.
- Glisser un dossier : vers une autre position des épinglés (même espace ou autre) uniquement ; un dépôt dans les favoris ou les onglets du jour est ignoré ; on ne peut pas déposer un dossier dans lui-même ou un de ses descendants.
- Créer : bouton `FolderPlus` toujours visible dans l'en-tête d'espace, action de la barre de commande `new-folder` « Nouveau dossier », et dans le menu contextuel d'un onglet épinglé ou du jour, « Ajouter à un dossier ▸ » (sous-menu : les dossiers de l'espace par leur chemin « Parent / Enfant », puis « Nouveau dossier »). Un nouveau dossier s'appelle « Nouveau dossier », est ouvert, et l'interface passe aussitôt son nom en édition (`folder:rename`).
- Menu contextuel d'un dossier (`showFolderMenu`) : Renommer, Nouveau dossier à l'intérieur, séparateur, Supprimer le dossier (ses éléments remontent à sa place dans le parent), Supprimer le dossier et ses onglets (confirmation, onglets détruits).
- Ligne de dossier : icône `Folder` / `FolderOpen`, nom, chevron discret au survol ; clic → `toggleFolder` ; double-clic sur le nom (qui rétablit d'abord l'état ouvert/fermé d'avant le premier clic : `toggleFolder(id, open)`) ou demande `folder:rename` → champ d'édition en place (Entrée valide, Échap annule, perte de focus valide ; nom vide → inchangé). Les enfants sont indentés de 14 px. Animation d'ouverture courte.
- Si l'onglet actif est dans un dossier fermé (ou un de ses sous-dossiers fermés), il s'affiche quand même juste sous la ligne du dossier fermé de plus haut niveau, indenté, pour qu'on voie toujours l'onglet actif.
- Glisser-déposer : sur une ligne de dossier, le quart haut = avant, le quart bas = après (au niveau du parent), le milieu = dans le dossier à la fin (`folderId`, `index = items.length`) avec la ligne surlignée. Dans un dossier ouvert, les indicateurs d'insertion portent son `folderId`.

### Import

Import des espaces, épinglés, dossiers et favoris depuis d'autres navigateurs installés, via des actions de la barre de commande qui n'apparaissent que si la source existe. Le format de chaque source et les règles de correspondance sont décrits dans `app/desktop/src/main/features/importers/FORMATS.md` : c'est le seul endroit du dépôt, avec le code de ce dossier et les libellés des actions d'import, où ces navigateurs sont nommés. 2 000 onglets maximum par import, créés déchargés.

### Boosts

Personnalisation par site, appliquée à tous les onglets (y compris Peek et Little Signet) dont `siteOf(url)` correspond.

- Store : `boosts: Record<site, Boost>`.
- Application : sur `dom-ready` de chaque onglet et à chaque modification d'un Boost, pour chaque vue concernée : `removeInsertedCSS(clé précédente)` puis, si le Boost est actif, `insertCSS(css + règles des éléments masqués)` en mémorisant la nouvelle clé (`Map<webContents.id, clé>`). Règle d'un élément masqué : `<sélecteur> { display: none !important; }`.
- Émojis en mode sombre : si le CSS contient le bloc `signet:dark`, ajouter `[data-signet-emoji] { filter: invert(1) hue-rotate(180deg) !important; }` et lancer `EMOJI_SCRIPT` (app/desktop/src/main/features/boost/emoji.ts, monde isolé 1007) qui entoure chaque émoji en couleur d'un `<span data-signet-emoji>` ; le nœud texte d'origine garde le début du texte, et un `MutationObserver` redécoupe quand la page le réécrit ou le retire. Sans le bloc, `EMOJI_STOP_SCRIPT` remet le texte d'origine. Ignorés : `script`, `style`, `textarea`, `option`, SVG, zones éditables.
- Ouvrir l'éditeur : action de la barre de commande `boost` « Personnaliser <site> » (sous-titre « Boost actif » s'il existe ; seulement si l'onglet actif est en http(s)), menu Affichage « Personnaliser ce site… » ⌥⌘B, et entrée « Personnaliser ce site… » dans le menu contextuel des pages. Une fenêtre par site, réutilisée si elle est déjà ouverte.
- Fenêtre : `BrowserWindow` 400×620, `minHeight 420`, `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 14, y: 14 }`, `vibrancy: 'under-window'`, route `#boost`, même preload. `boost:state` envoyé au chargement (avec file d'attente comme les autres vues) et à chaque changement.
- Interface `#boost` : en-tête en `drag` (marge gauche 76 px) « Boost » + site ; interrupteur Activé ; puces de préréglages qui ajoutent ou retirent un bloc de CSS délimité par des commentaires `/* signet:<id> */ … /* /signet:<id> */` : « Mode sombre » (`html { filter: invert(1) hue-rotate(180deg); } img, video, picture, canvas, iframe, svg image { filter: invert(1) hue-rotate(180deg); }`), « Police système » (`* { font-family: -apple-system, system-ui, sans-serif !important; }`), « Largeur de lecture » (`body { max-width: 860px !important; margin-inline: auto !important; }`), « Sans animations » (`*, *::before, *::after { animation: none !important; transition: none !important; }`) ; zone CSS en police monospace (Tab insère deux espaces, enregistrement automatique 400 ms après la dernière frappe via `saveBoost`) ; section « Éléments masqués » avec le bouton « Masquer des éléments » (icône `Crosshair`, désactivé si `hasTab` est faux) qui devient « Terminer (Échap) » pendant `zapping`, et la liste des sélecteurs avec ✕ ; en bas « Supprimer le Boost » avec confirmation en place.
- Outil « Masquer » : cible = l'onglet actif de la fenêtre principale si son site correspond, sinon le premier onglet chargé du site. Le main met la fenêtre principale au premier plan et active cet onglet, puis appelle en boucle `executeJavaScriptInIsolatedWorld(1001, [{ code }], true)` : le script affiche un surlignage (contour de 2 px et voile léger qui suivent l'élément survolé, petite étiquette avec la balise), intercepte le prochain clic en phase de capture (`preventDefault`, `stopPropagation`) et renvoie une promesse résolue avec un sélecteur, ou `null` sur Échap ou sur appel de `window.__signetZapStop()`. Chaque sélecteur est ajouté (sans doublon), appliqué aussitôt et poussé dans l'état, puis on relance. `stopZap` exécute `window.__signetZapStop?.()` dans le même monde isolé. Construction du sélecteur : `#id` si l'id est unique et n'a pas l'air généré (pas de suite de 4 chiffres ou plus) ; sinon une chaîne d'au plus 5 ancêtres, chacun `balise` + jusqu'à 2 classes stables (sans chiffres ni motif de hachage comme `css-1x2y3z`) + `:nth-of-type()` si nécessaire, jusqu'à ce que `document.querySelectorAll(sélecteur).length === 1`.

### Icône de l'app

- Dessin : défini dans `app/desktop/scripts/icons/generate.mjs`. Un signet (ruban de livre à queue fendue) pend du bord haut, à gauche, avec trois lignes de texte à droite : la barre latérale et la page. Le ruban part au-dessus du canevas et le squircle le coupe, pour qu'il touche toujours le bord haut, même sur les icônes mobiles agrandies ou réduites. Ne pas changer le dessin, seulement les couleurs.
- Variantes (fond : 3 arrêts du dégradé diagonal ; ruban : 3 arrêts de gauche à droite, avec un reflet blanc ; lignes : couleur et opacité) :
  - `aurore` (par défaut) : fond `#2a3864` `#1f2a4b` `#141b33` ; ruban `#f0645e` `#d63a3f` `#b42a33`, reflet .22 ; lignes `#4a5c8a` ; ombre `#060a18` .50, lueur .10
  - `ocean` : fond `#2563eb` `#0891b2` `#2dd4bf` ; ruban `#fde68a` `#fbbf24` `#f59e0b`, reflet .35 ; lignes blanches .50 ; ombre `#0c2d5e` .40, lueur .22
  - `corail` : fond `#fb923c` `#f43f5e` `#e11d74` ; ruban `#ffffff` `#fff1f4` `#ffe1e8` ; lignes blanches .50 ; ombre `#7f1d1d` .40, lueur .22
  - `menthe` : fond `#059669` `#10b981` `#a3e635` ; ruban `#ffffff` `#effdf5` `#d9f9e6` ; lignes blanches .50 ; ombre `#064e3b` .40, lueur .22
  - `nuit` : fond `#0b0b14` `#1e1b4b` `#312e81` ; ruban `#c4b5fd` `#a78bfa` `#8b5cf6`, reflet .30 ; lignes `#6366f1` .70 ; ombre noire .50, lueur .08
  - `papier` : fond `#ffffff` `#f8f7fb` `#efedf5` ; ruban comme `aurore` ; lignes `#cfcbdc` ; ombre `#1d1d1f` .18, pas de lueur, liseré noir à .06
  - `graphite` : fond `#52525b` `#27272a` `#18181b` ; ruban `#fafafa` `#e4e4e7` `#a1a1aa` ; lignes `#71717a` ; ombre noire .45, lueur .08
  - `or` : fond `#1c1917` `#292524` `#44403c` ; ruban `#fde68a` `#f59e0b` `#b45309`, reflet .30 ; lignes `#78716c` ; ombre noire .45, lueur .08
- Génération : `npm run icons` (scripts dans `app/desktop/scripts/icons/`) écrit `app/desktop/resources/icons/svg/<id>.svg` depuis le dessin, rend `resources/icons/<id>.png` en 1024×1024 (rendu par Electron hors écran : `BrowserWindow` invisible, `capturePage`, fond transparent), et produit `build/icon.icns` (variante `aurore`) avec `iconutil` à partir d'un `.iconset` (16, 32, 128, 256, 512 en @1x et @2x). Les fichiers générés sont commités.
- Empaquetage : `mac.icon` = `build/icon.icns` ; `extraResources` copie `resources/icons/*.png` dans `icons/`.
- Exécution : au démarrage et à chaque changement de `settings.appIcon`, `app.dock?.setIcon(nativeImage.createFromPath(...))` ; dossier `join(app.getAppPath(), 'resources/icons')` en développement, `join(process.resourcesPath, 'icons')` dans l'app empaquetée.
- Choix : actions `icon:<id>` « Icône de l'app : <Nom> », sous-titre « Actuelle » pour la variante active, `image` = aperçu 64 px en data URL (calculé une fois, `nativeImage.resize` puis `toDataURL`). Elles apparaissent quand la saisie contient « icône », « icone », « icon » ou le nom d'une variante, pas avec une saisie vide. La barre de commande affiche `image` en 22 px aux coins arrondis à la place de l'icône d'action.
- Limite connue (macOS) : l'icône du Finder et du Launchpad reste la variante par défaut ; seule l'icône du Dock change pendant l'exécution.

## Réglages

Fenêtre unique (réutilisée si elle est déjà ouverte) : `BrowserWindow` 780×600, `minWidth 640`, `minHeight 460`, `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 16, y: 18 }`, `vibrancy: 'sidebar'`, route `#settings`, même preload. Ouverture : menu Signet › « Réglages… » ⌘, ; action `settings` « Réglages » de la barre de commande ; bouton ⚙ (`Settings`) dans la barre du bas de la barre latérale, à gauche des espaces.
Nouveaux canaux : `settings:open`, `settings:get`, `settings:update`, `settings:default-browser`, `settings:import`, `settings:open-boost`, `settings:clear-data` ; événement poussé `settings:info` (au chargement de la page, avec file d'attente, et à chaque `changed()` tant que la fenêtre est ouverte ; ignorer les envois identiques au précédent).
`updateSettings` fusionne le patch, applique les effets (bloqueur de pubs, icône du Dock…) en réutilisant le code existant, puis `changed()`. Les actions équivalentes de la barre de commande restent en place.

Interface `#settings` : colonne de navigation à gauche (200 px, en `drag` en haut avec la place des feux tricolores, fond transparent pour laisser la vibrancy) et contenu à droite sur fond uni, titre de section en haut. Sections :

- **Général** : « Archiver les onglets du jour après » (sélecteur : 6 h, 12 h, 24 h, 3 jours, 7 jours, jamais = 0 → le main n'archive plus si 0) ; interrupteur « Ouvrir dans Peek les liens externes des onglets épinglés » ; « Navigateur par défaut » : état + bouton « Définir par défaut » (désactivé avec une note « Disponible dans l'app installée » si `packaged` est faux).
- **Apparence** : grille des icônes de l'app (aperçus 64 px, nom dessous, la sélection a un anneau de la teinte) ; note discrète « L'icône du Dock change tout de suite ; celle du Finder reste Aurore. »
- **Confidentialité** : interrupteur « Bloquer les pubs et les traqueurs » ; bloc « Effacer les données » avec cases à cocher (Historique — n entrées, Onglets archivés — n, Liste des téléchargements — n, Cache, Cookies et données des sites) et un bouton « Effacer… » qui demande confirmation en place. Côté main : historique vidé et sauvegardé, archive vidée, liste des téléchargements vidée sauf ceux en cours, `session.defaultSession.clearCache()`, `clearStorageData()` pour les cookies et données des sites.
- **Import** : une ligne par source avec son nom, « Importer » si `available`, sinon « Introuvable sur ce Mac » en gris ; une note sur l'accès complet au disque que certaines sources demandent. Ce sont les seuls libellés de cette fenêtre qui nomment d'autres navigateurs (ils viennent de `importSources`, pas du code de l'interface).
- **Boosts** : liste des Boosts (site, actif ou non, nombre d'éléments masqués, date), interrupteur par ligne (`saveBoost(site, { enabled })`), bouton « Modifier » (`openBoost`), bouton supprimer (`deleteBoost`, confirmation en place). État vide : explication et raccourci ⌥⌘B.
- **Raccourcis** : tableau en lecture seule de tous les raccourcis, groupés (Onglets, Navigation, Espaces, Barre latérale, Vue partagée, Recherche, Fenêtres). La liste vient d'un tableau constant dans l'interface, à garder synchronisé avec menu.ts.
- **À propos** : grande icône de la variante active, « Signet », version, lien vers le dépôt https://github.com/ItsKarmaOff/Signet (ouvert dans un nouvel onglet de la fenêtre principale).

Style cohérent avec le reste (mode sombre, lucide-react, survols doux), contrôles natifs-like : interrupteurs arrondis, sélecteurs, boutons secondaires.

## v4 — refonte visuelle sobre

Remplace le style « dégradé coloré » par une interface neutre et calme. La règle d'or : la couleur est un accent, jamais un fond. Supprimer **tous** les dégradés de teinte (barre latérale, fenêtre principale, Little Signet, barre flottante, Boost, Réglages, overlay). Les règles de cette section priment sur les descriptions visuelles des sections précédentes ; le comportement, lui, ne change pas.

### Couleurs

Jetons CSS uniques dans `:root`, redéfinis en sombre (`prefers-color-scheme: dark`) :

- `--bg` fond de fenêtre : laisser passer la vibrancy, avec un voile neutre (clair : `rgb(236 236 239 / .72)` ; sombre : `rgb(44 43 48 / .72)`).
- `--surface` cartes et panneaux (clair `#ffffff`, sombre `#1f1f23`), `--surface-2` survols et tuiles (clair `rgb(0 0 0 / .045)`, sombre `rgb(255 255 255 / .06)`), `--border` (clair `rgb(0 0 0 / .08)`, sombre `rgb(255 255 255 / .08)`).
- `--text` (clair `#1d1d1f`, sombre `#ececf0`), `--text-2` secondaire (clair `#6e6e73`, sombre `#9b9aa3`), `--text-3` tertiaire.
- `--accent` : `hsl(var(--h) 70% 58%)` en clair, `hsl(var(--h) 75% 68%)` en sombre. Sert uniquement aux accents (focus, anneau de sélection, interrupteurs, point de l'espace actif).
- `--active` fond de l'onglet actif : sombre `rgb(0 0 0 / .38)`, clair `#ffffff` avec `box-shadow: 0 1px 2px rgb(0 0 0 / .08), 0 0 0 .5px rgb(0 0 0 / .06)`.
- Fenêtres : `vibrancy: 'sidebar'` pour la principale et les Little Signet (au lieu de `under-window`).

### Mise en page de la fenêtre principale

- Colonne de contenu à droite de la barre latérale : une **barre d'outils de 44 px** en haut (sur le fond de fenêtre, zone `drag`), puis la **carte web** en dessous. Le main décale les vues : `area.y = 44`, `area.height = H - 44 - 8`. Toute la carte web (une ou plusieurs vues) garde ses 4 coins arrondis (rayon 10).
- Barre d'outils (dans le renderer de la fenêtre principale, boutons en `no-drag`) : à gauche, bouton barre latérale (seulement quand elle est masquée, après 78 px réservés aux feux tricolores), Précédent, Suivant, Recharger (croix pendant le chargement) ; puis l'adresse sous la forme « **domaine** / titre de la page » (domaine en `--text`, « / titre » en `--text-3`, tronqué ; cadenas discret si https ; clic → `openPalette('edit-url')`). À droite : bouton Boost (icône `Rocket`, accent si un Boost est actif pour ce site), et rien d'autre. En vue partagée, la barre d'outils suit le panneau qui a le focus.
- Barre latérale (fond `--bg`, pas de carte) :
  1. En-tête de 44 px en `drag` : 78 px pour les feux tricolores, nom de l'espace actif en gras 13 px (sans emoji) (clic droit → menu de l'espace), puis à droite le bouton téléchargements (anneau de progression, point non vu) et le bouton « Nouveau dossier » au survol.
  2. Pas de pilule d'URL ni de boutons de navigation dans la barre latérale (ils sont dans la barre d'outils).
  3. Favoris : grille de **3 colonnes**, tuiles de 44 px de haut, rayon 10, fond `--surface-2`, liseré `--border`, favicon 18 px centré ; tuile active en `--active`.
  4. Épinglés, avec les dossiers en **cartes teintées** : fond `hsl(hue 60% 50% / .16)` (sombre) ou `/ .12` (clair), rayon 10, padding 4 px ; en-tête : icône de dossier, nom en couleur `hsl(hue 70% 62%)` (sombre) ou `hsl(hue 60% 42%)` (clair), chevron ; enfants à l'intérieur. Sans `hue` : fond `--surface-2`, nom en `--text`. Sous-dossiers : même principe, sans double fond si même teinte.
  5. Séparateur fin `--border` (même écart au-dessus et en dessous ; sans épinglés, la liste vide ne prend aucune place et les favoris perdent leur marge du bas), puis « + Nouvel onglet » en `--text-2`, puis les onglets du jour.
  6. Ligne d'onglet : 32 px, rayon 8, favicon 16 px, titre 13 px `--text` ; survol `--surface-2` ; actif `--active`. Bouton fermer au survol uniquement.
  7. Barre du bas (36 px) : bouton Réglages (engrenage, `--text-3`) à gauche ; au centre, **l'emoji de chaque espace** (SpaceStrip.tsx : cases de 28 px, emoji 15 px atténué, l'espace actif en pleine couleur sur un fond `--sel`, survol → bulle avec le nom) ; à droite « + » nouvel espace. Les emojis restent des zones de dépôt. Au plus 5 emojis par page (4 si la rangée fait moins de 140 px) : au-delà, la rangée se range en pages qu'on fait glisser au trackpad, comme les espaces (suivi 1:1, butée nette à la première et à la dernière page, fondu de 10 px du côté où une autre page attend). Quand l'espace actif change, sa page vient à l'écran avec un ressort qui part de la position affichée (interruptible par un geste). Une seule page : le geste sur la rangée change d'espace comme ailleurs dans la barre.
- Fond de la fenêtre et des zones vides : `--bg`. État vide de la carte web : carte `--surface` avec le texte et le bouton « Nouvel onglet ⌘T ».

### Couleur des dossiers

- Nouveau champ `hue` (optionnel) sur les dossiers. Menu contextuel d'un dossier : sous-menu « Couleur ▸ » avec Aucune, puis 8 teintes nommées (Rouge 0, Orange 28, Jaune 48, Vert 140, Turquoise 180, Bleu 215, Violet 265, Rose 330), coche sur la teinte actuelle. Un nouveau dossier n'a pas de teinte. Les dossiers importés non plus.

### Autres vues

- Barre de commande : panneau `--surface` (sombre légèrement translucide avec flou), liseré `--border`, ombre douce, aucune teinte en fond ; ligne sélectionnée en `--surface-2` avec une barre d'accent de 2 px à gauche.
- Peek : voile `rgb(0 0 0 / .45)`, boutons ronds `--surface`.
- Little Signet : barre de 44 px sur `--bg` (vibrancy), pas de dégradé.
- Barre latérale flottante : fond `--surface` presque opaque, liseré `--border`, ombre.
- Barre de recherche, formulaire d'espace, sélecteur d'emoji, panneau des téléchargements, Boost, Réglages : mêmes jetons, aucune teinte de fond. L'accent reste pour les interrupteurs, les anneaux de sélection et le focus.

### Vérification visuelle

Étendre `SIGNET_CAPTURE` (dossier `app/desktop/src/main/dev/` : `capture.ts`, `seed.ts`, `probes.ts`) pour capturer la fenêtre principale **avec un état d'exemple** : si `SIGNET_SEED=1`, avant d'ouvrir la fenêtre, remplacer l'état en mémoire (jamais sur le profil normal : seulement si `--user-data-dir` est passé) par 2 espaces, 4 favoris, des épinglés dont un dossier teinté bleu ouvert de 3 onglets et un dossier rouge de 1 onglet, 5 onglets du jour dont un actif, sur des sites publics simples (wikipedia.org, example.com, github.com, news.ycombinator.com, lemonde.fr, w3.org). Captures en clair et en sombre (`nativeTheme.themeSource`).

## v5 — compact, médias, vue partagée avancée, espaces avancés, bibliothèque

Quatre lots développés en parallèle par quatre personnes. Le contrat (`app/desktop/src/shared/types/`) est déjà étendu pour tous ; chaque lot n'implémente que ses méthodes de `SignetApi` dans le preload et `index.ts`. Migration du store sans changer `version` : `settings.compactMode` → `false`, `settings.autoPip` → `true`, `settings.routes` → `[]`, `sitePermissions` → `{}`, `Snapshot.toolbarFloating` → `false`, `dividers` → `[]`, `media` → `null`, `boostedSites` → calculé. `pruneItems` (tree.ts) doit conserver `layout`, `sizes` des vues partagées et `customTitle`, `customIcon` des onglets.
Canaux : nom de la méthode en kebab-case préfixé du domaine (`compact:toggle`, `toolbar:hover`, `media:toggle`, `media:skip`, `drag:begin`, `drag:end`, `drag:drop`, `split:layout`, `split:sizes`, `space:profile`, `route:add`, `route:remove`, `library:open`, `history:query`, `history:delete`, `archive:list`, `archive:restore`, `archive:delete`, `tab:rename`, `tab:icon`, `site:panel`, `site:permission`, `site:clear`) ; événements poussés `library:section`, `tab:rename-request`, `tab:icon-request`.

Répartition des fichiers (pour limiter les conflits) :

- Lot A : app/desktop/src/main/browser/compact.ts, app/desktop/src/main/browser/media.ts, app/desktop/src/renderer/src/ui/MediaCard.tsx, route `#toolbar`.
- Lot B : packages/shared/src/split.ts (calcul des rectangles, séparateurs, insertion), app/desktop/src/renderer/src/tabs/DropZones.tsx, app/desktop/src/renderer/src/tabs/Dividers.tsx.
- Lot C : app/desktop/src/main/web/profiles.ts (sessions), app/desktop/src/main/web/permissions.ts, packages/shared/src/routes.ts, app/desktop/src/renderer/src/lib/useSpaceSwipe.ts, section « Aiguillage » des Réglages.
- Lot D : app/desktop/src/main/windows/library.ts, app/desktop/src/main/browser/sitepanel.ts, app/desktop/src/renderer/src/windows/Library.tsx, app/desktop/src/renderer/src/windows/SitePanel.tsx, route `#library`.
  Fichiers partagés (à l'époque : browser.ts, tabs.ts, index.ts, preload, store.ts, palette.ts, menu.ts, Sidebar.tsx, TabRow.tsx, App.tsx, Toolbar.tsx, Settings.tsx, styles.css ; aujourd'hui le gros de browser.ts est réparti dans `main/browser/`) : uniquement des Edit ciblés.

### Lot A — Mode compact, médias, image dans l'image

**Mode compact** (`settings.compactMode`). Bascule : menu Affichage « Mode compact » ⌥⌘S, action de la barre de commande `compact`, interrupteur dans Réglages › Général. ⌘S en mode compact quitte le mode compact et affiche la barre latérale.

- Layout : barre latérale et barre d'outils cachées, feux tricolores cachés ; la carte web occupe `{ x: 6, y: 6, width: W - 12, height: H - 12 }`.
- Bord gauche : la barre latérale flottante existante (inchangée).
- Bord haut : une bande de 6 px en haut de la fenêtre (dans le renderer principal, au-dessus de la carte) appelle `setToolbarHover(true)`. Le main affiche alors une vue `#toolbar` (même composant `Toolbar`, fond `--surface` presque opaque, liseré, ombre) en `{ x: carte.x + 8, y: 6, width: carte.width - 16, height: 40 }`, rayon 10, et réaffiche les feux tricolores. `setToolbarHover(false)` (mouseleave de la vue) la cache après 250 ms, annulable. Ordre de pile : au-dessus des onglets, sous l'overlay.
- Transitions : la carte web change de taille d'un coup (vues natives) ; la barre d'outils flottante apparaît avec une translation de 6 px + fondu (--t) et repart par le même chemin ; la barre latérale flottante glisse depuis le bord gauche (`translateX(-100%)` → 0 dans les bornes de sa vue) et y retourne. Le main garde la vue posée pendant la sortie (200 ms, 120 ms en mouvement réduit) ; un retour du pointeur l'annule et repart de la position courante (transitions CSS).

**Médias** (`Snapshot.media`).

- Suivi par onglet : `media-started-playing`, `media-paused`, `audio-state-changed`. Un onglet est candidat s'il joue, ou s'il a joué et est en pause depuis moins de 10 minutes. `media` = le candidat le plus récemment actif **qui n'est pas affiché** ; `null` sinon.
- Métadonnées : à chaque changement d'état, `executeJavaScriptInIsolatedWorld(1002, …)` lit `navigator.mediaSession.metadata` (titre, artiste, dernière pochette) ; à défaut, titre de l'onglet. `canSkip` = des métadonnées MediaSession existent.
- `mediaToggle` : `executeJavaScript(code, true)` (geste utilisateur) qui met en pause le média qui joue, ou relance le dernier média en pause. `mediaSkip` : `sendInputEvent` des touches `MediaNextTrack` / `MediaPreviousTrack` (keyDown puis keyUp), au mieux.
- Interface `MediaCard` : carte compacte juste au-dessus de la barre du bas de la barre latérale : pochette 32 px (sinon favicon), titre et artiste sur deux lignes tronquées, boutons précédent (si `canSkip`), lecture/pause, suivant (si `canSkip`), couper le son. Clic sur la carte → `activateTab`. Apparition en glissant de 8 px vers le haut + fondu.

**Image dans l'image automatique** (`settings.autoPip`, interrupteur dans Réglages › Général). Quand un onglet quitte l'affichage alors qu'une de ses vidéos joue : AVANT de retirer sa vue, `executeJavaScript` avec geste utilisateur qui appelle `requestPictureInPicture()` sur la vidéo qui joue (si aucune n'est déjà en PiP). Quand l'onglet revient à l'affichage : `document.exitPictureInPicture()` si sa vidéo est en PiP. Erreurs ignorées.

### Lot B — Vue partagée avancée

- Jusqu'à **4** onglets par vue partagée (au lieu de 3). Dispositions : `columns` (côte à côte), `rows` (empilée), `grid` (deux rangées : `ceil(n/2)` en haut, le reste en bas ; seulement pour n ≥ 3, sinon équivaut à `columns`).
- Rectangles (split.ts, fonction pure testable) : à partir de la zone de contenu, de `layout`, `sizes` et `GAP = 8`. Taille minimale d'un panneau : 200 px (clamp, en redistribuant). `Snapshot.dividers` : en `columns`, `n - 1` séparateurs verticaux (bande de 8 px entre deux panneaux, `index` = frontière entre la part `index` et `index + 1`) ; en `rows`, horizontaux ; en `grid`, un seul séparateur horizontal entre les rangées (`index` 0) — les colonnes d'une rangée restent égales.
- Interface `Dividers` (fenêtre principale, sous les vues) : chaque séparateur est une zone de prise de 8 px (`cursor: col-resize` / `row-resize`) ; au survol, une poignée de 3×28 px arrondie apparaît au centre ; pendant le glisser, elle prend l'accent. Glisser = suivi 1:1 du pointeur (Pointer Events + `setPointerCapture`, en respectant le décalage de prise), `setSplitSizes` envoyé au plus une fois par frame (`requestAnimationFrame`). Double-clic = parts égales.
- **Glisser un onglet sur la page** : au `dragstart` d'une ligne d'onglet de la barre latérale (onglet seul, pas de dossier ni de vue partagée entière ; un favori ne peut faire que `center`), l'interface appelle `beginTabDrag(tabId)`. Le main ouvre l'overlay en mode `dropzones` (`area` = rectangle de la carte web). `DropZones` reçoit les événements de glisser HTML5 (même type MIME que la barre latérale) : bandes de 25 % sur chaque bord → `left` / `right` / `top` / `bottom`, centre → `center`. Pendant le survol, un aperçu translucide (`--accent` à 18 %, liseré accent, rayon 10) montre la place que prendra l'onglet, et il glisse vers la nouvelle zone avec une transition de 140 ms. Dépôt → `dropOnContent(tabId, zone)`. `dragend` dans la barre latérale ou Échap → `endTabDrag()` ferme l'overlay.
- Sémantique du dépôt : `center` → active l'onglet. Bords : l'onglet rejoint la vue partagée de l'onglet affiché (en la créant au besoin), inséré avant (left/top) ou après (right/bottom) le panneau qui a le focus. `left`/`right` → disposition `columns` (si elle était `rows` et que n ≥ 3 après ajout → `grid`) ; `top`/`bottom` → `rows` (si elle était `columns` et n ≥ 3 → `grid`). Les parts sont remises à égalité. Si la vue partagée a déjà 4 onglets → comme `center`. Si l'onglet affiché est un favori → comme `center`.
- Changer de disposition : menu contextuel d'une vue partagée dans la barre latérale « Disposition ▸ Côte à côte / Empilée / Grille » (Grille seulement si n ≥ 3), actions de la barre de commande `split-columns`, `split-rows`, `split-grid`, `split-equalize` « Égaliser les panneaux ». La ligne de vue partagée de la barre latérale affiche une petite icône de disposition (`Columns2` / `Rows2` / `Grid2x2`).
- La barre de recherche dans la page et Peek suivent les nouveaux rectangles.

### Lot C — Espaces avancés

**Profils** (`Space.profile`).

- `isolated` → partition `persist:space-<spaceId>` ; `shared` (défaut) → session par défaut. Les favoris, Peek ouvert depuis un favori et Little Signet (hors aiguillage) utilisent la session par défaut ; un onglet utilise la session de son espace ; Peek utilise celle de l'onglet d'où il vient.
- profiles.ts : `sessionFor(spaceId | null)` et `configureSession(ses)`, appelée une fois par session : user agent, gestionnaire d'autorisations (permissions.ts), bloqueur de pubs (adblock.ts doit gérer plusieurs sessions), téléchargements (downloads.ts doit écouter `will-download` sur chaque session). Remplacer les usages directs de `session.defaultSession` par ces fonctions.
- Changer le profil d'un espace (`setSpaceProfile`) : boîte de confirmation (« Les onglets de cet espace vont se recharger. Les comptes connectés ne seront pas partagés avec les autres espaces. ») puis détruire les vues de ses onglets (rechargées à l'affichage). Déplacer un onglet vers un espace de profil différent détruit sa vue de la même façon.
- Formulaire d'espace : interrupteur « Profil isolé » avec une phrase d'explication. Menu de l'espace : « Profil isolé » coché ou non, et « Effacer les données de l'espace… » (profil isolé seulement : `clearStorageData` + `clearCache` de sa partition, confirmation). En-tête de la barre latérale : petite icône `UserRound` à côté du nom des espaces isolés (avec `title` explicatif).

**Autorisations persistantes** (permissions.ts, utilisé aussi par le lot D). Classe `Permissions` sur `store.state.sitePermissions: Record<origin, Record<permission, 'allow' | 'deny'>>` : `get(origin, permission)`, `set(origin, permission, state: PermissionState)` (`ask` = supprimer la décision), `list(origin)`. Le gestionnaire de demande consulte ces décisions avant d'afficher la boîte de dialogue et y enregistre la réponse (au lieu de la mémoire de session). Ajouter aussi `setPermissionCheckHandler` qui répond selon ces décisions. Le bloqueur de pubs compte les requêtes bloquées par `webContents` (événement `request-blocked` de la bibliothèque), remis à zéro à chaque navigation du cadre principal : méthode `blockedCount(webContentsId)` dans adblock.ts.

**Aiguillage** (`settings.routes`, routes.ts). `matchRoute(url)` : hôte égal au motif ou sous-domaine de celui-ci ; `*` dans le motif = n'importe quelle suite de caractères sauf `/`. S'applique aux nouveaux onglets ouverts depuis la barre de commande (mode `new-tab`) et aux liens externes (`open-url`, second lancement) : si une règle désigne un autre espace, l'onglet s'ouvre dans cet espace (qui devient actif) dans la fenêtre principale, au lieu de l'espace actif ou d'une Little Signet. Réglages : nouvelle section « Aiguillage » (icône `Signpost`) : liste des règles (motif → emoji + nom de l'espace, bouton supprimer), formulaire d'ajout (champ motif + sélecteur d'espace). Menu contextuel d'un onglet : « Toujours ouvrir <site> dans ▸ » (sous-menu des espaces).

**Glissement entre espaces** (useSpaceSwipe.ts, dans la barre latérale et la barre flottante) :

- Événements `wheel` dont `|deltaX| > |deltaY|` sur la barre latérale (hors champs de texte). Suivi 1:1 : la piste des listes d'espaces (précédent, actuel, suivant, chacun de la largeur de la barre ; les favoris restent fixes) suit `deltaX` cumulé. Butée nette au premier et au dernier espace : pousser vers l’extérieur ne déplace rien (ni élastique ni retour).
- Fin du geste : plus d'événement depuis 90 ms. Vitesse = moyenne des derniers déplacements (px/s). Projection : `position + (v / 1000) * 0.998 / (1 - 0.998)`. Si la projection dépasse la moitié de la largeur → animation à ressort (amortissement 1, réponse 0,35 s, vitesse initiale = vitesse du geste) vers l'espace voisin puis `switchSpace` ; sinon retour à ressort. Petit utilitaire `spring.ts` dans le renderer (ressort critique intégré par `requestAnimationFrame`, interruptible : un nouveau geste repart de la position affichée).
- `prefers-reduced-motion` : pas de suivi, bascule au seuil avec un fondu de 150 ms.
- La mise en avant des emojis d'espace en bas (fond et opacité) suit la progression du geste.
- BrowserWindow `swipe` (geste à trois doigts, s'il est activé dans macOS) → espace précédent / suivant.

### Lot D — Bibliothèque, renommer les onglets, panneau du site

**Bibliothèque** : fenêtre unique `#library` (comme Réglages : 920×660, navigation à gauche en vibrancy, contenu à droite). Ouverture : ⌘Y, action `library` « Bibliothèque », bouton `Library` dans la barre du bas de la barre latérale (à côté de l'engrenage), `openLibrary(section)` (pousse `library:section` si déjà ouverte).

- Historique : recherche (insensible aux accents), liste groupée par jour (« Aujourd'hui », « Hier », puis « mardi 21 octobre »), ligne = favicon, titre, domaine, heure ; clic → nouvel onglet dans la fenêtre principale ; ✕ au survol → `deleteHistory` ; chargement par pages de 100 au défilement ; bouton « Effacer l'historique… » (confirmation, `clearBrowsingData(['history'])`).
- Archives : recherche, groupées par jour, « Restaurer » (onglet du jour dans l'espace actif, retiré des archives) et ✕.
- Téléchargements : liste complète (mêmes lignes que le panneau de la barre latérale), Effacer.
- Espaces : une carte par espace (emoji, nom, pastille de teinte, nombre d'épinglés et d'onglets du jour, badge profil isolé) ; clic → `switchSpace` et fenêtre principale au premier plan ; bouton « Modifier » → formulaire d'espace dans la barre latérale (`space:edit`).

**Renommer un onglet / lui donner un emoji** : double-clic sur le titre d'une ligne d'onglet → champ en place (Entrée valide, Échap annule, vide = revenir au titre de la page) → `renameTab`. Menu contextuel de l'onglet : « Renommer… » (`tab:rename-request`), « Choisir une icône… » (`tab:icon-request` → la barre latérale ouvre l'`EmojiPicker` existant dans un panneau, comme le formulaire d'espace → `setTabIcon`), « Rétablir le nom et l'icône » si personnalisé. Affichage : `customIcon` à la place du favicon (emoji 15 px), `customTitle` à la place du titre dans la barre latérale, la barre de commande et les tuiles de favoris (`title`). La barre d'outils garde le vrai titre de la page.

**Panneau du site** : clic sur le cadenas (ou l'icône d'info pour http) de la barre d'outils → `openSitePanel(anchor)` → le main calcule `SiteInfo` de l'onglet actif et ouvre l'overlay en mode `site`. `SitePanel` : carte de 320 px sous l'ancre (flèche discrète), sans voile mais clic à l'extérieur ou Échap ferme. Contenu : en-tête (favicon, site, « Connexion sécurisée » / « Connexion non sécurisée ») ; autorisations Caméra, Micro, Position, Notifications, Presse-papiers avec un contrôle segmenté Demander / Autoriser / Bloquer (`setSitePermission`, via la classe `Permissions` du lot C : `media` est stocké séparément pour `camera` et `microphone` si possible, sinon une seule ligne « Caméra et micro ») ; « N éléments bloqués sur cette page » (via `blockedCount` du lot C) ; ligne Boost (interrupteur + « Modifier ») ; pied « Effacer les cookies et données du site » (`clearStorageData({ origin })` sur la session de l'onglet, puis rechargement, confirmation en place).
**Indicateur Boost** : `Snapshot.boostedSites` ; la fusée de la barre d'outils prend `--accent` quand le site affiché y figure.

### Principes d'interaction (pour tous les lots)

Tirés des recommandations d'Apple pour des interfaces fluides :

- Retour visuel au `pointerdown`, jamais seulement au clic : lignes, tuiles et boutons s'enfoncent légèrement (`scale(.985)`, 80 ms).
- Tout ce que l'utilisateur fait glisser suit le pointeur 1:1, en respectant le point de prise, et reste interruptible.
- Animations courtes et sans rebond par défaut (ressort critique) ; un léger rebond seulement après un geste avec élan.
- Entrée et sortie par le même chemin ; les panneaux naissent de l'élément qui les ouvre (`transform-origin`).
- `prefers-reduced-motion` : fondus courts à la place des translations.
- Formule élastique : `rubberband(d, dim, c = 0.55) = (d * dim * c) / (dim + c * |d|)`.

## v6 — finition du design

But : passer d'une interface « propre » à une interface soignée, calme et vivante. La v4 a posé la sobriété (aucun dégradé) ; cette passe travaille la géométrie, la matière, la typographie et le mouvement. Elle s'applique à tout : fenêtre principale, barres flottantes, barre de commande, Peek, panneau du site, zones de dépôt, Little Signet, Boost, Réglages, Bibliothèque. Le comportement ne change pas.

### Géométrie

- **Rayons concentriques** : un élément imbriqué a le rayon de son parent moins l'espacement qui les sépare. Jetons : `--r-window: 16px` (coin des fenêtres macOS), `--gap: 8px` (espacement entre la barre latérale, la barre d'outils et la carte web), `--r-card: calc(var(--r-window) - var(--gap) / 2)` = 12 px pour la carte web (donc `setBorderRadius(12)` côté main pour les vues d'onglet et de Peek), `--r-row: 8px` pour les lignes de la barre latérale, `--r-tile: 10px` pour les tuiles de favoris et les cartes de dossier, `--r-control: 7px` pour les boutons et champs, `--r-pop: 14px` pour la barre de commande, le panneau du site et les panneaux flottants.
- **Coins en superellipse** sur tout ce qui est dessiné en CSS : `corner-shape: superellipse(1.3)` (le Chromium d'Electron 45 le gère ; sans effet ailleurs). Pas sur les vues natives.
- **Grille de 4 px** : tous les espacements, hauteurs et marges sont des multiples de 4 (exceptions justifiées : traits de 1 px, `--gap` partagé). Ligne d'onglet 32 px, tuile de favori 44 px, barre d'outils 44 px, barre du bas 36 px.
- Alignements : les icônes de la barre latérale (favicons, icônes de dossier, « + Nouvel onglet ») sont toutes sur la même colonne ; les titres sur une même ligne de départ.

### Matière et profondeur

- **La carte web flotte** : ombre douce dessinée par le renderer derrière chaque panneau (un élément exactement sous la vue native) : `0 1px 2px rgb(0 0 0 / .10), 0 3px 8px rgb(0 0 0 / .18)` en sombre, `0 1px 2px rgb(0 0 0 / .06), 0 3px 8px rgb(0 0 0 / .10)` en clair. Liseré intérieur de 0,5 px `--border` autour de la carte.
- **Neutres légèrement teintés** (pas de dégradé) : `--bg`, `--surface`, `--surface-2` et `--border` reçoivent 2 à 4 % de la teinte de l'espace via `color-mix(in oklch, …)`. Changement d'espace : transition de couleur de 240 ms.
- **Couches flottantes** (barre de commande, panneau du site, barres flottantes, menus de l'interface) : `backdrop-filter: blur(24px) saturate(160%)`, fond `--surface` à 82 % (sombre) ou 78 % (clair), arête haute plus claire (`box-shadow: inset 0 0.5px 0 rgb(255 255 255 / .12)` en sombre), ombre plus forte que la carte web car plus haute : `0 12px 32px rgb(0 0 0 / .28), 0 2px 6px rgb(0 0 0 / .16)`. Ne jamais empiler deux couches translucides claires.
- **Voile** seulement pour les tâches modales (barre de commande : `rgb(0 0 0 / .18)` ; Peek : `.45`). Le panneau du site et les zones de dépôt n'ont pas de voile.
- **Bord de défilement** : quand la liste de la barre latérale défile sous l'en-tête ou au-dessus de la barre du bas, un masque en fondu de 12 px (`mask-image`) au lieu d'un trait.

### Typographie

- Police système (`-apple-system`, `system-ui`), `font-optical-sizing: auto`, lissage `-webkit-font-smoothing: antialiased`.
- Échelle : 11 px (légendes, raccourcis, compteurs) `letter-spacing: .01em` ; 12 px (sous-titres) ; 13 px (texte courant, lignes d'onglet) poids 450 ; 13 px poids 600 (nom de l'espace, titres de section de la barre latérale) ; 20 px poids 650 `letter-spacing: -0.01em` (titres des Réglages et de la Bibliothèque) ; 28 px poids 700 `-0.02em` (À propos).
- Hiérarchie par le poids et la couleur (`--text`, `--text-2`, `--text-3`) plutôt que par la taille. Chiffres tabulaires (`font-variant-numeric: tabular-nums`) pour les tailles, heures et compteurs.
- Titres d'onglet : `--text` pour l'onglet actif, `--text-2` pour les autres (un peu plus lisibles au survol).

### Mouvement

- Jetons d'animation dans `:root` : `--ease-out: cubic-bezier(.2, .8, .2, 1)` ; ressort critique en CSS via `linear()` : `--spring: linear(0, .17 4%, .48 10%, .72 17%, .87 25%, .95 34%, .985 45%, 1)` (réponse ≈ 0,35 s, sans rebond) ; durées `--t-fast: 120ms`, `--t: 200ms`, `--t-slow: 320ms`, `--t-hud: 150ms` (fondus HUD / quitter / changement d'espace) ; graisses `--fw-body` 450, `--fw-medium` 500, `--fw-label` 550, `--fw-strong` 600, `--fw-title` 650, `--fw-hero` 700 ; matière des grandes surfaces modales (barre de commande) : `--blur-lg: blur(40px) saturate(170%)` et `--shadow-modal` (plus profonde que `--shadow-pop`). `prefers-contrast: more` et `prefers-reduced-transparency` rendent `--glass`, `--glass-view` opaques et neutralisent `--blur` / `--blur-lg` (contraste : aussi `--edge-light`).
- **Retour au toucher** : `:active` → `scale(.985)` en 80 ms sur lignes, tuiles, boutons, emojis d'espace ; survol en `--t-fast`.
- **Apparitions** : la barre de commande et le panneau du site naissent de leur origine (barre : léger `scale(.98)` + `translateY(-4px)` + fondu ; panneau du site : `transform-origin` sur le cadenas) avec `--spring`, et repartent par le même chemin. Les dossiers s'ouvrent en animant la hauteur (`grid-template-rows: 0fr → 1fr`) et la rotation du chevron.
- **Changement d'onglet actif** : la pastille de l'onglet actif glisse vers sa nouvelle position quand c'est dans la même liste (élément unique positionné en absolu qui suit la ligne active, avec `--spring`) ; sinon simple fondu.
- **Téléchargement** : au début d'un téléchargement, une petite pastille (favicon ou icône de fichier) part du centre de la carte web et décrit un arc jusqu'au bouton des téléchargements (`offset-path` ou deux transformations composées), 600 ms, puis le bouton fait un léger rebond (seul rebond de l'app, justifié par l'arrivée d'un objet). L'événement vient du main (`downloads` qui s'allonge).
- **Ajout et retrait d'onglets** : la ligne apparaît en dépliant sa hauteur de 0 à 32 px avec un fondu (`--t`), et disparaît de même.
- `prefers-reduced-motion: reduce` : fondus de 120 ms à la place des translations, échelles et arcs ; pas d'animation de la pastille active. `prefers-reduced-transparency: reduce` : couches flottantes opaques, sans flou. `prefers-contrast: more` : `--border` à 20 %, texte secondaire plus foncé.

### Détails à soigner

- Focus clavier visible et cohérent : anneau de 2 px `--accent` décalé de 2 px (`outline-offset`), seulement en navigation clavier (`:focus-visible`).
- Infobulles natives (`title`) sur tous les boutons d'icône, avec le raccourci entre parenthèses quand il existe.
- Barre de défilement fine et discrète dans la barre latérale (largeur 6 px, visible au défilement seulement).
- États vides soignés (Bibliothèque, Boosts, téléchargements, recherche sans résultat) : une icône `--text-3` de 28 px, une phrase, une action si utile.
- Aucune couleur codée en dur hors des jetons ; aucune valeur d'espacement hors de la grille de 4 px sans raison.

## v7 — barre d'outils fondue dans le site

La barre d'outils n'est plus une bande au-dessus de la page, sur le fond de la fenêtre : elle fait partie de la **carte** de chaque panneau et prend **la couleur du haut du site**, si bien que la barre et la page ne forment qu'une seule surface. Contrat : `TabInfo.themeColor`, `Pane.card`, `LittleState.themeColor`.

### Mise en page

- La colonne de contenu ne réserve plus de bande de 44 px en haut de la fenêtre. Chaque panneau est une carte : `card` = son rectangle dans la zone de contenu (qui commence à `y = 8`, comme la barre latérale qui garde son en-tête de 44 px avec les feux tricolores) ; `rect` (la vue web) = `card` moins la hauteur de la barre de panneau (`PANE_BAR = 40`) en haut.
- Le calcul des rectangles de la vue partagée (split.ts) travaille sur les cartes ; les séparateurs restent entre les cartes. Les zones de dépôt couvrent les cartes. La barre de recherche dans la page et Peek se placent par rapport à `rect`.
- Vues web : `setBorderRadius(12)` inchangé. Leurs coins hauts arrondis se posent sur la barre du panneau, de la même couleur : la jonction est invisible.
- Barre latérale masquée : la carte la plus à gauche réserve 78 px à gauche de sa barre pour les feux tricolores.
- Mode compact : la carte occupe toute la fenêtre moins 6 px, **sans** barre de panneau (vue web = carte) ; la barre d'outils flottante existante reste le moyen de naviguer, et elle prend elle aussi la couleur du site.

### Barre de panneau (renderer de la fenêtre principale, dessinée dans `card` au-dessus de la vue)

- Fond = `themeColor` de l'onglet du panneau (transition de couleur de 200 ms, sans animation si `prefers-reduced-motion`) ; à défaut, `--surface`. Coins hauts de la carte arrondis à 12, avec la même ombre de carte qu'avant (l'ombre entoure toute la carte, barre comprise).
- Couleur du contenu calculée par contraste : le blanc contraste mieux que le noir sur `themeColor` (rapports WCAG) → texte et icônes clairs (`rgb(255 255 255 / .92)`, secondaire `/ .55`) ; sinon foncés (`rgb(0 0 0 / .85)`, secondaire `/ .5`). Survol des boutons : la couleur du contenu à 10 %.
- Contenu, de gauche à droite : Précédent, Suivant, Recharger ; puis, sans pilule ni fond, le cadenas (ou l'icône d'info) suivi de « **domaine** / titre » (titre en couleur secondaire, tronqué) — clic sur le cadenas → panneau du site, clic sur le texte → modifier l'adresse ; à droite la fusée Boost (accent si actif).
- Toute la barre est une zone `drag` (la fenêtre se déplace en la saisissant), les boutons et le texte cliquable en `no-drag`.
- Vue partagée : chaque panneau a sa propre barre, qui agit sur son onglet (les boutons ciblent `pane.tabId`). Le panneau qui a le focus garde son anneau ; un panneau sans focus voit sa barre à 70 % d'opacité de contenu. Si la carte fait moins de 420 px de large : seulement Recharger, le cadenas et le domaine.
- Plus aucune barre d'outils globale au-dessus des panneaux (Toolbar.tsx devient la barre d'un panneau, réutilisée aussi par la barre flottante du mode compact).

### Couleur du site (main)

- Pour chaque onglet affiché : échantillonner le haut de la page avec `webContents.capturePage({ x: 0, y: 0, width, height: 2 })` (largeur de la vue), puis prendre la couleur la plus fréquente des pixels de la première ligne (quantifiée sur 4 bits par canal pour ignorer l'anticrénelage), en ignorant les pixels totalement transparents. Format `#rrggbb`.
- Quand : `did-finish-load`, `did-navigate-in-page`, `dom-ready` + 300 ms, quand l'onglet devient affiché, puis toutes les 1,5 s tant qu'il est affiché et que la fenêtre est visible (la couleur peut changer au défilement ou au passage clair/sombre). Ne mettre à jour `themeColor` (et donc pousser l'état) que si la couleur a changé.
- Si l'échantillonnage échoue ou que la page est vide : utiliser la dernière couleur `did-change-theme-color` (meta `theme-color`) si elle existe, sinon laisser `themeColor` indéfini.
- Little Signet : `LittleState.themeColor` ; sa barre de 44 px prend la couleur du site avec le même calcul de contraste. Barre flottante du mode compact : idem.

### Vérification

`SIGNET_SEED=1` + captures : impossible de voir la vraie page dans une capture de la fenêtre (vues natives) ; pour vérifier la fusion, le crochet de capture doit, en mode capture seulement, composer l'image de la fenêtre avec les `capturePage()` des vues web aux positions `rect`. Vérifier au moins : une page claire (wikipedia.org), une page sombre (un site à fond noir), une vue partagée avec deux couleurs différentes, la barre latérale masquée (feux tricolores sur la barre), en clair et en sombre.

## v8 — langue des sites et extensions

Licence du projet : GPL-3.0-or-later (fichier LICENSE). Contrat : `Settings.languages`, `ExtensionInfo`, `SettingsInfo.extensions`, `openExtensionStore`, `removeExtension`. Migration : `settings.languages`, s'il est absent ou vide, est calculé au chargement à partir des langues du système (`app.getPreferredSystemLanguages()`, chaque code régional suivi de sa langue de base, sans doublon : `en-US, fr-FR` → `en-US, en, fr-FR, fr`) ; repli `['en-US', 'en']` si le système n'en donne aucune.

### Langue des sites

- Les sites doivent recevoir les langues de `settings.languages` dans l'en-tête `Accept-Language` et dans `navigator.language` / `navigator.languages`.
- En-tête : pour **chaque** session (par défaut et profils isolés, voir profiles.ts `configureSession`), `ses.setUserAgent(userAgent, acceptLanguages)` avec la liste simple `fr-FR,fr,en-US,en` (Chromium ajoute lui-même les poids : `fr-FR,fr;q=0.9,en-US;q=0.8,en;q=0.7`). Les valeurs par défaut suivent le système, pas une langue fixée. Vérifier qu'aucun autre appel à `setUserAgent` n'écrase les langues.
- `navigator.language` / `languages` : Chromium les tire de la langue du système et ignore `setUserAgent` ; chaque vue d'onglet reçoit donc `Emulation.setUserAgentOverride` (protocole de débogage, `emulateLanguages`) avec la même liste, et avec `userAgentMetadata` : les indices natifs de Chromium, relevés une fois dans la vue de l'interface principale (non surchargée) dès son chargement (`captureNativeHints` : marques avec la marque GREASE propre à la version, ex. « Not:A-Brand » v8 pour 156, version complète, plateforme et sa version, architecture), puis réappliqués aux onglets déjà ouverts. Ne jamais les inventer : des indices faux ne ressemblent à aucun Chromium réel. Sans ces métadonnées, la surcharge efface les indices client (`Sec-CH-UA`, `navigator.userAgentData.brands` vide) : un « Chrome » sans indices passe pour un navigateur intégré et Google refuse la connexion. Le menu Develop garde l'identité par défaut tant qu'aucun agent n'est choisi. Connexion Google : refusée malgré tout (« Ce navigateur ou cette application ne sont peut-être pas sécurisés »), y compris en s'annonçant comme Firefox sur accounts.google.com (essayé puis retiré : Google repère Chromium dans la page et, probablement, par l'empreinte TLS de la connexion). Comparaison avec CEF 156 pur (accepté par Google, prototype Rust cef-rs `cefsimple`) : écarts relevés côté Electron — version complète dans l'agent (corrigé), indices inventés par Signet (corrigé), aucun indice client sur les navigations ni indices détaillés via `Accept-CH`, `window.chrome` vide (CEF : `app`, `csi`, `loadTimes`), notifications « granted » d'office. En complément, `app.commandLine.appendSwitch('lang', languages[0])` avant `ready` (lu depuis state.json) règle la langue de l'application ; un changement de la première langue s'applique entièrement au prochain lancement (le dire dans l'interface).
- Changement de la liste : réappliquer l'en-tête à toutes les sessions tout de suite, puis recharger les onglets affichés.
- Réglages › Général, groupe « Langues des sites » : liste ordonnée (glisser ou flèches haut/bas), bouton supprimer par langue (au moins une), ajout via un sélecteur des langues courantes (nom en français : Français, English, Español, Deutsch, Italiano, Português, Nederlands, 日本語, 한국어, 中文…, plus les variantes régionales utiles fr-FR, fr-CA, fr-BE, en-US, en-GB…), bouton « Revenir aux langues du système » (envoie une liste vide, que le processus principal remplace par les langues du système), note « La langue principale s'applique entièrement au prochain lancement ».

### Extensions

Bibliothèques (déjà installées, dans `dependencies`) : `electron-chrome-extensions` (API d'extension : onglets, fenêtres, boutons d'action, menus contextuels…) et `electron-chrome-web-store` (installation et mises à jour depuis la boutique). Tout le code qui les utilise vit dans `app/desktop/src/main/features/extensions/extensions.ts` (et le preload pour les boutons d'action) : c'est l'exception validée à la règle « aucun nom d'autre navigateur » ; les libellés de l'interface restent neutres (« Extensions », « Boutique d'extensions »). Lire leur README et leurs types dans node_modules avant de coder (si une règle de permission en bloque la lecture, utiliser leur dépôt public github.com/samuelmaddock/electron-browser-shell via `gh`).

- Extensions stockées dans `<userData>/Extensions` ; chargées dans **chaque** session (par défaut et profils isolés) au démarrage et à la création d'une session.
- Une instance `ElectronChromeExtensions` par session, avec les rappels requis : créer un onglet (nouvel onglet du jour dans l'espace actif, ou dans l'espace de la session), sélectionner un onglet, fermer un onglet, créer / fermer une fenêtre (Little Signet pour les fenêtres popup, sinon onglet). Chaque vue d'onglet est déclarée à la bibliothèque à sa création (`addTab`) et à son activation (`selectTab`).
- Boutique : `installChromeWebStore` pour chaque session, de sorte que le bouton « Ajouter » de la boutique fonctionne dans Signet et que le bandeau qui demande un autre navigateur disparaisse. Mises à jour automatiques activées.
- Boutons d'action : dans la barre de chaque panneau, à droite, juste avant la fusée Boost, les boutons des extensions qui en ont (élément fourni par la bibliothèque, injecté par le preload de la fenêtre principale ; adapter la configuration electron-vite pour que le preload embarque ce code, car un preload `sandbox` ne peut pas `require` de module). Ils suivent la couleur de contenu de la barre (`--bar-fg`) autant que possible. Les popups d'extension s'ouvrent sous leur bouton.
- Menus contextuels : les entrées ajoutées par les extensions apparaissent dans le menu contextuel des pages (tabs.ts).
- Réglages : nouvelle section « Extensions » (icône `Puzzle`) : liste des extensions installées (icône, nom, version, bouton « Retirer » avec confirmation en place), bouton « Ouvrir la boutique d'extensions » (`openExtensionStore` → nouvel onglet sur la boutique dans la fenêtre principale), et une note honnête : « La plupart des extensions fonctionnent. Certaines, qui ont besoin de fonctions avancées (réseau, proxy, identité), peuvent ne pas marcher. »
- Le bloqueur de pubs intégré reste indépendant.

## v9 — interface en anglais et en français

Toute l'interface de Signet devient traduisible. Langues disponibles au départ : anglais et français. Par défaut, l'interface suit la langue de macOS (`app.getPreferredSystemLanguages()` : la première langue prise en charge, sinon l'anglais). L'utilisateur peut forcer une langue dans les Réglages. Plus aucun texte visible écrit en dur dans le code.

### Mécanique (déjà en place)

- Bibliothèques : `i18next` (dependencies, utilisé par le main et l'interface) et `react-i18next` (devDependencies, intégré à l'interface par Vite).
- packages/shared/src/i18n.ts : `createI18n(locale, espaceParDéfaut)` crée une instance i18next indépendante, `resolveLocale(préférence, languesSystème)`, `LOCALES`, `resources`. app/desktop/src/shared/i18next.d.ts type les clés (TypeScript signale une clé inconnue).
- Catalogues JSON standards, éditables par des contributeurs ou des outils de traduction : `packages/shared/src/locales/<en|fr>/main.json` (processus principal : menu d'application, menus contextuels natifs, boîtes de dialogue, actions et sous-titres de la barre de commande, résumés d'import, titres de fenêtres) et `ui.json` (interface React). L'anglais est la référence ; une clé absente en français retombe sur l'anglais, mais le français doit être complet (contrôlé par le script de vérification).
- Syntaxe i18next : clés imbriquées `zone.élément` en anglais (`sidebar.newTab`, `settings.general.title`, `palette.action.pin`, `dialog.deleteSpace.message`), interpolation `{{nom}}`, pluriels par suffixes `_one` / `_other` avec `count`. Pas de clé générique réutilisée pour des sens différents.

### Contrat

- `Settings.uiLanguage?: UiLanguage` (absent = `system`).
- `SignetApi.getLocale(): Promise<Locale>` et `SignetApi.onLocale(cb)` : toutes les vues (fenêtre principale, overlay, barres flottantes, recherche, Little Signet, Boost, Réglages, Bibliothèque) lisent la langue au montage et se mettent à jour en direct quand elle change.

### Main

- `app/desktop/src/main/app/i18n.ts` : langue courante (préférence + langues système), `t()`, changement de langue → reconstruire le menu d'application, pousser `locale` à tous les `webContents` de l'interface, recalculer ce qui en dépend (sous-titres de la barre de commande au prochain appel).
- Tous les textes visibles du main passent par `t()` : menu d'application (y compris les libellés des rôles Édition et Fenêtre), menus contextuels des pages, des onglets, des espaces, des dossiers, des vues partagées, boîtes de dialogue (autorisations, confirmations, import, profils), actions et sous-titres de la barre de commande, titres de fenêtres.
- Le nom des sous-dossiers créés par l'import (« Autres favoris »…) suit la langue au moment de l'import.

### Interface

- `I18nextProvider` (react-i18next) à la racine de chaque route (main.tsx), avec une instance créée par `createI18n(locale, 'ui')` ; `useTranslation()` dans les composants ; changement de langue → `i18n.changeLanguage()`.
- Tous les textes visibles (libellés, infobulles `title`, `aria-label`, textes de remplacement, états vides, confirmations) passent par `t()`.
- Dates, heures, tailles et nombres : `Intl` avec la langue courante (Bibliothèque, téléchargements, Boosts). Les noms de jours et « Aujourd'hui / Hier » viennent du catalogue ou d'`Intl.RelativeTimeFormat`.
- Sélecteur d'emoji : données emojibase de la langue courante (`emojibase-data/en/…` ou `fr/…`), recherche dans cette langue.
- Réglages › Général, en tête : « Langue de l'interface » : « Système (English) » / « English » / « Français » ; le changement s'applique immédiatement partout.
- Le tableau des raccourcis des Réglages est traduit aussi.

### Vérification

- Script de contrôle (dans app/desktop/scripts/, lancé par `npm run i18n:check`) qui vérifie que fr et en ont exactement les mêmes clés et qui échoue s'il reste une chaîne visible en dur dans `app/desktop/src/renderer/src/**` et dans les libellés du main (heuristique : chaînes JSX de texte et attributs `title`, `aria-label`, `placeholder`, `label:` contenant des lettres, hors clés, noms propres listés explicitement comme « Signet », « Little Signet », « Peek », « Boost »). Le lancer et le laisser propre.
- Captures (crochet de développement, profil isolé) de la fenêtre principale, de la barre de commande si possible, des Réglages et de la Bibliothèque, en anglais et en français.

## v10 — glisser pour naviguer, menu des extensions

Contrat : `Settings.swipeNavigation?` (absent = activé), `SignetApi.showExtensionMenu(id)`.

### Glisser à deux doigts pour revenir en arrière ou aller en avant

Le moteur d'Electron n'inclut pas la navigation par balayage du trackpad : on la reconstruit.

- **Script de page** (app/desktop/src/preload/swipe.ts, entrée séparée du preload construite par electron-vite en un seul fichier autonome, compatible `sandbox`) enregistré sur chaque session avec `ses.registerPreloadScript({ type: 'frame', … })` (profiles.ts `configureSession`) ; il n'agit que dans le cadre principal des onglets (pas dans les vues de l'interface, ni les popups d'extension).
- Détection, sur les événements `wheel` (écoute passive, en capture sur `window`) : geste horizontal (`|deltaX| > 1,5 × |deltaY|` sur les premiers événements, verrouillé ensuite pour tout le geste) ET aucun élément sous le pointeur (ni la page) ne peut encore défiler horizontalement dans ce sens (remonter les ancêtres : `overflow-x` auto/scroll et `scrollLeft` pas en butée) ET l'événement n'a pas été annulé par la page (`defaultPrevented` vérifié dans un écouteur posé après celui de la page, ex. via `setTimeout(0)` ou en écoute non capturante sur `window`). Fin du geste : plus d'événement depuis 150 ms (les événements d'élan du trackpad prolongent le geste), mais on ne l'attend pas pour décider : les pages n'ont pas la phase du geste, donc on détecte le relâchement par l'élan (|deltaX| qui décroissent, 3 événements en baisse avec dernier ≤ 0,9 × premier après le seuil, 4 avant le seuil). Seuil franchi + élan détecté (ou déplacement ≥ 2,5 × seuil) → navigation immédiate ; élan détecté avant le seuil → la bulle revient aussitôt. Ensuite le reste du flux est ignoré jusqu'à 150 ms de silence, sauf une hausse nette de |deltaX| (3 événements) qui signale de nouveaux doigts. Un début de flux déjà décroissant (élan arrivant sur la page rechargée) n'ouvre pas de geste. Pause du main après une navigation par glissement : 400 ms. Mesuré (SIGNET_SWIPECHECK, doigts + 1 s d'inertie) : dernier doigt → navigation ≈ 1,2 s avant, ≈ 100-140 ms après (dont ≈ 25 ms jusqu'au début de navigation).
- Sens : glisser vers la droite (contenu tiré vers la droite, `deltaX` négatif) = précédent ; vers la gauche = suivant. Au début d'un geste, le script demande au main (`ipcRenderer.invoke('swipe:can')`) si l'onglet peut reculer / avancer ; sans historique dans ce sens, il montre une résistance (la bulle avance à peine, formule élastique) et ne navigue pas.
- **Indicateur** (dessiné par le script dans la page, dans un `shadow root` fermé pour ne subir aucun style de la page, `position: fixed`, `pointer-events: none`, z-index maximal) : une bulle ronde de 40 px avec une flèche, qui entre depuis le bord gauche (précédent) ou droit (suivant) et suit le doigt 1:1 jusqu'au seuil (120 px de déplacement cumulé), avec une légère résistance au-delà ; au seuil, la bulle se remplit de l'accent et grossit légèrement (retour visuel « ça va naviguer »). Au relâchement : au-delà du seuil → `ipcRenderer.send('swipe:navigate', 'back' | 'forward')` et la bulle s'efface vers l'extérieur ; sinon elle revient en arrière avec un ressort. Respect de `prefers-reduced-motion` (fondu).
- Main : `swipe:can` et `swipe:navigate` résolvent l'onglet depuis `event.sender` ; navigation via `navigationHistory`. Ignorer si `settings.swipeNavigation === false` (le script reçoit le réglage via `swipe:can`).
- Réglages › Général : interrupteur « Glisser à deux doigts pour naviguer » (traduit en et fr).
- Ne pas casser : défilement horizontal des pages (carrousels, tableaux larges, cartes), le glissement entre espaces sur la barre latérale (autre vue), les gestes à trois doigts du système.

### Extensions épinglées et bouton Extensions

- Seules les extensions épinglées (`settings.pinnedExtensions`) ont un bouton dans la barre d'outils, dans l'ordre `extensionOrder` (glisser-déposer). Une extension installée, importée ou restaurée n'est jamais épinglée d'office ; à la mise à jour qui a introduit l'épinglage (remplaçant `hiddenExtensions`, « tout affiché sauf les masquées »), toutes les extensions sont désépinglées.
- Bouton Extensions (pièce de puzzle, après les boutons épinglés, présent dès qu'une extension a un bouton) → `showExtensionsMenu()` → menu natif : chaque extension avec un bouton (icône 16 px, nom ; clic = son popup s'ouvre sous le bouton Extensions, l'activation se fait côté interface avec le rectangle de ce bouton), séparateur, sous-menu « Épinglées dans la barre d'outils » (cases à cocher), « Gérer les extensions… ».
- Réglages › Extensions : l'interrupteur de chaque extension avec un bouton l'épingle ou la désépingle.

### Menu des boutons d'extension

- Clic droit sur un bouton d'extension de la barre → `showExtensionMenu(id)` → menu natif traduit : nom de l'extension (grisé), « Options » (si l'extension déclare une page d'options : l'ouvrir dans un nouvel onglet), « Désépingler de la barre d'outils », « Gérer les extensions » (Réglages › Extensions), séparateur, « Retirer de Signet… » (boîte de confirmation native puis retrait de toutes les sessions, comme `removeExtension`). Si la bibliothèque fournit déjà un menu contextuel pour ses boutons, le remplacer par celui-ci.
- Récupérer l'identifiant de l'extension sous le pointeur depuis l'élément `browser-action-list` (lire comment la bibliothèque rend ses boutons : attributs, parties `::part`, éléments dans le shadow DOM) ; si ce n'est pas possible proprement côté interface, passer par l'API de la bibliothèque côté main.

## v11 — adresses internes `signet://`

Un schéma d'adresses interne, comme ceux des autres navigateurs. Tout le code vit dans `app/desktop/src/main/web/internal.ts` (seul fichier, avec la liste des pages techniques du moteur, autorisé à contenir les adresses internes du moteur — exception de nommage, comme les importeurs).

### Trois familles d'adresses

1. **Raccourcis vers les écrans de Signet** (rien ne s'ouvre dans un onglet ; si on les tape dans la barre de commande en mode nouvel onglet, aucun onglet vide n'est créé) :
   - `signet://settings` → Réglages ; `signet://settings/<section>` → la section (`general`, `appearance`, `privacy`, `import`, `boosts`, `routing`, `extensions`, `shortcuts`, `about`) ;
   - `signet://extensions` = `signet://settings/extensions` ; `signet://about` = `signet://settings/about` ;
   - `signet://library` → Bibliothèque ; `signet://library/<history|archive|downloads|spaces>` ; `signet://history`, `signet://downloads` = les sections correspondantes.
   - Section inconnue → la fenêtre s'ouvre sur sa première section.
2. **Pages d'information servies dans un onglet** par `protocol.handle('signet', …)` sur chaque session (schéma enregistré avant `ready` avec `protocol.registerSchemesAsPrivileged` : `standard: true, secure: true`, sans accès particulier) : HTML statique généré, traduit avec `t()` du main, mis en forme proprement (jetons de couleur clair / sombre de l'app, police système, `Content-Security-Policy` stricte sans script) :
   - `signet://urls` : la liste de toutes les adresses `signet://` disponibles, cliquables, groupées par famille, avec une phrase de description chacune ;
   - `signet://version` : version de Signet, d'Electron, du moteur, de Node, de V8, système, langue de l'interface et langues des sites, agent utilisateur, dossier du profil, dossier des extensions ;
   - adresse `signet://` inconnue → page « Adresse introuvable » qui renvoie vers `signet://urls`.
3. **Pages techniques du moteur**, réexposées sous `signet://<nom>` : vérifier une par une lesquelles se chargent réellement dans Electron 45 (candidates : gpu, media-internals, webrtc-internals, process-internals, accessibility, serviceworker-internals, indexeddb-internals, quota-internals, histograms, net-export, tracing, ukm, private-aggregation-internals, attribution-internals, topics-internals) et ne garder que celles qui marchent. `signet://gpu` charge la page du moteur correspondante dans l'onglet.

### Affichage et saisie

- Une page technique du moteur s'affiche **partout** sous son adresse `signet://` (barre d'outils, barre latérale, historique, barre de commande, Bibliothèque) : une fonction `displayUrl(url)` dans le main fait la conversion pour l'affichage et une `resolveInternal(url)` la conversion inverse pour la navigation. L'adresse réelle du moteur ne doit jamais apparaître dans l'interface.
- `toUrl()` reconnaît `signet://…` (et `signet:…` sans les barres) comme une adresse.
- Barre de commande : dès que la saisie commence par `signet:`, proposer les adresses internes correspondantes (titre = adresse, sous-titre = description traduite) avant tout le reste.
- Les liens `signet://` cliqués dans une page `signet://` fonctionnent (y compris les raccourcis vers les écrans).
- Sécurité : une page web ordinaire ne peut pas naviguer vers `signet://` ni vers une page technique du moteur (refuser dans `will-navigate` / `setWindowOpenHandler` si l'origine d'où part la navigation n'est pas `signet:` ou une saisie de l'utilisateur).

## v12 — onglets en haut (⌃⇧S)

Deuxième disposition, au choix : la barre latérale (par défaut) ou une **rangée d'onglets en haut de la fenêtre**, avec la page en dessous. Bascule par ⌃⇧S (menu Affichage « Onglets en haut » / « Onglets sur le côté », action de la barre de commande, Réglages › Apparence). Contrat : `Settings.tabLayout`. Le comportement (espaces, favoris, épinglés, dossiers, vues partagées, glisser-déposer, menus contextuels, renommage, médias, téléchargements) est le même ; seule la présentation change.

### Mise en page (main)

- Disposition `horizontal` : pas de barre latérale (ni flottante) ; une **barre d'onglets** de 40 px en haut de la fenêtre (renderer de la fenêtre principale, zone `drag`), puis les cartes des panneaux en dessous, avec les marges habituelles (`x = 8`, `y = 40 + 4`). Les feux tricolores restent visibles, centrés verticalement dans la barre d'onglets (`trafficLightPosition` ajustée selon la disposition).
- Mode compact + disposition horizontale : la barre d'onglets se cache et réapparaît au survol du bord haut (comme la barre d'outils flottante, dans la même vue flottante ou une vue dédiée) ; les cartes occupent toute la fenêtre.
- Changer de disposition ne recharge aucune page.

### Barre d'onglets (renderer)

De gauche à droite, après 78 px pour les feux tricolores :

1. **Sélecteur d'espace** : bouton (emoji + nom de l'espace, tronqué à 120 px) ; clic → menu de l'interface (pas natif) listant les espaces (emoji, nom, coche sur l'actif), « Nouvel espace », « Modifier l'espace » ; un formulaire d'espace s'ouvre en popover sous le bouton (le popover doit passer au-dessus des pages : utiliser l'overlay existant ou une vue dédiée, pas le DOM de la fenêtre principale qui est sous les vues web).
2. **Favoris** puis **épinglés** : pastilles carrées de 28 px avec le favicon (ou l'emoji personnalisé), sans titre ; infobulle avec le titre. Un dossier épinglé = une pastille icône de dossier (teinte du dossier) ; clic → menu déroulant listant ses onglets (et sous-dossiers en sous-menus), dans la même couche au-dessus des pages.
3. Séparateur fin.
4. **Onglets du jour** : pastilles favicon + titre, largeur entre 120 et 220 px, qui rétrécissent quand il y en a beaucoup puis défilent horizontalement (molette, avec fondus sur les bords) ; bouton fermer au survol ; l'onglet actif en `--active`. Une vue partagée = une pastille qui regroupe les favicons de ses onglets (et une icône de disposition), clic → l'affiche.
5. Bouton **+** (nouvel onglet ⌘T).
6. À droite : carte média compacte (si un média joue ailleurs : favicon/pochette + lecture/pause), bouton téléchargements (anneau de progression), Bibliothèque, Réglages.

Glisser-déposer : réordonner dans la rangée (indicateur d'insertion vertical de 2 px), déposer sur une pastille de dossier pour y ranger, sur le sélecteur d'espace pour envoyer vers un espace (le menu s'ouvre au survol prolongé), et vers la page pour la vue partagée (zones de dépôt existantes). Mêmes appels au main que la barre latérale (`moveItem`, `beginTabDrag`…).

Clic droit, clic milieu, double-clic (renommer) : comme dans la barre latérale. Glissement à deux doigts sur la barre d'onglets : défilement horizontal de la rangée (pas de changement d'espace dans cette disposition).

Style : mêmes jetons que la v6 ; barre d'onglets sur `--bg` (vibrancy), pastilles en `--surface-2` au survol, rayon 8, hauteur 30 px, animations courtes (apparition / disparition d'un onglet par la largeur), `prefers-reduced-motion` respecté.

### Vérification

Captures (crochet de développement, profil isolé, `SIGNET_SEED=1`) en disposition horizontale, en clair et en sombre : avec favoris, épinglés dont un dossier, 6 onglets du jour dont l'actif et une vue partagée ; puis avec 25 onglets (rétrécissement et défilement) ; puis le menu du sélecteur d'espace ouvert et le menu d'un dossier ouvert. Vérifier aussi l'aller-retour ⌃⇧S sans rechargement de page.

## v13 — mini-lecteur flottant (image dans l'image maison)

La fenêtre d'image dans l'image intégrée au moteur est basique et non personnalisable. On la remplace par notre propre mini-lecteur, sur le modèle de l'image dans l'image moderne des navigateurs : fenêtre flottante aux coins arrondis, contrôles translucides au survol. Contrat : `PipState`, `PipCommand`, `onPipState`, `pipCommand`, `openPip`. Code : app/desktop/src/main/windows/pip.ts, route `#pip` (app/desktop/src/renderer/src/windows/Pip.tsx).

### Principe

- La **vraie page** de l'onglet est déplacée dans une petite fenêtre et seule sa vidéo reste visible : le script (monde principal, `executeJavaScript`) marque la vidéo qui joue, l'extrait **synchrone­ment** de son parent vers un conteneur `position: fixed; inset: 0; z-index: 2147483647; background: #000` ajouté au `body` (la lecture continue car le déplacement a lieu dans la même tâche), avec `object-fit: contain`, `width/height: 100%`, et masque les barres de défilement (`html, body { overflow: hidden }`). Au retour, la vidéo est remise exactement à sa place d'origine (parent + voisin suivant mémorisés) et le style injecté retiré (`removeInsertedCSS`).
- **Fenêtre** : `BrowserWindow` sans cadre (`frame: false`, `roundedCorners: true`, `hasShadow: true`), `alwaysOnTop` niveau `floating`, `visibleOnAllWorkspaces` (y compris par-dessus les apps en plein écran), `type: 'panel'` sur macOS, pas dans le Dock ni dans ⌘Tab, non redimensionnable par le système (`resizable: false`, pas de `setAspectRatio` : avec des proportions verrouillées, macOS ne fait suivre qu'un axe et le bord s'écarte du pointeur) ; redimensionnée à la main par des poignées invisibles de la vue `#pip` (4 coins de 14 px, 4 bords de 8 px), proportions de la vidéo (`videoWidth / videoHeight`, 16:9 par défaut) conservées. Canal hors contrat `pip:resize` : début / déplacement absolu du pointeur depuis le début du geste / fin ; le main recalcule la géométrie à chaque fois depuis le rectangle de départ, de sorte que le bord ou le coin saisi reste sous le pointeur (coin : projection sur la diagonale de proportions, coin opposé fixe ; bord : centré sur l'autre axe). Largeur valide entre 240 px et 90 % de la zone de travail, mais ces deux bornes sont élastiques : un seul chemin pour les 8 poignées (largeur brute voulue → `rubberband(dépassement, dimension, 0.55)` hors bornes → hauteur par les proportions → position par l'ancre), la fenêtre continue donc de suivre le pointeur avec une résistance croissante, et au relâchement un ressort critique (réponse 0,4 s, lancé avec la dernière vitesse de redimensionnement) la ramène à la taille valide la plus proche, ancre fixe ; une nouvelle saisie interrompt ce retour depuis le rectangle réel. Le bord de l'écran reste une borne dure (la fenêtre ne le franchit jamais). Mouvement réduit : bornes dures, pas de retour animé. Pas d'aimantation ni de mémorisation pendant le geste ; la taille mémorisée est la taille d'arrivée, jamais le dépassement. Taille par défaut 400 px de large ; position par défaut en bas à droite de l'écran de la fenêtre principale, à 16 px des bords ; dernière taille et position mémorisées (store). La vue de l'onglet occupe toute la fenêtre avec `setBorderRadius(12)` ; une vue transparente `#pip` est posée par-dessus pour les contrôles.
- **Glisser** la fenêtre depuis n'importe où hors des boutons (zone `drag` de la vue `#pip`) ; au relâchement près d'un coin (moins de 48 px), elle s'aimante au coin à 16 px de marge avec une animation courte (`setBounds` animé côté main par petites étapes ou `animate: true`).

### Contrôles (vue `#pip`, fond transparent)

Invisibles au repos ; au survol, un voile `rgb(0 0 0 / .35)` apparaît en 150 ms avec :

- en haut à gauche : bouton « Revenir à l'onglet » (icône flèche diagonale vers le haut-gauche) dans une pastille carrée translucide (`rgb(255 255 255 / .16)` + flou, rayon 10) ;
- en haut au centre : le domaine du site (`www.` retiré), blanc, 13 px, poids 600, une ligne tronquée ;
- en haut à droite : pastille « × Fermer » (icône + texte, traduit) ;
- au centre : recul de 15 s (icône flèche circulaire avec « 15 »), lecture / pause (grande icône 40 px, barres arrondies), avance de 15 s ; espacement généreux ; chaque bouton s'enfonce légèrement au clic ;
- en bas : barre de progression épaisse (5 px, rayon complet, piste `rgb(255 255 255 / .35)`, progression blanche), cliquable et glissable pour avancer ou reculer, qui grossit légèrement au survol ; masquée pour un direct (`duration` infinie ou nulle) ; un bouton son (couper / rétablir) en bas à droite, discret.
- Raccourcis quand la fenêtre a le focus : Espace = lecture / pause, ← / → = ±15 s, M = son, Échap = fermer.
- Tout passe par `pipCommand` ; l'état (`PipState`) est relu dans la page toutes les 500 ms tant que le mini-lecteur est ouvert (lecture, position, durée, son) et poussé à la vue.

### Quand il s'ouvre et se ferme

- **Automatiquement** (réglage existant `settings.autoPip`) : quand on quitte l'affichage d'un onglet dont une vidéo joue **avec du son** (`heard`, voir media.ts), à la place de `requestPictureInPicture`. Quand l'onglet redevient affiché dans la fenêtre principale, le mini-lecteur se ferme et la page revient dans sa carte, vidéo remise en place, sans interruption de la lecture.
- **En quittant la fenêtre** (même réglage) : quand la fenêtre principale perd le focus (autre app, autre bureau, autre fenêtre de Signet, réduction) et qu'un onglet affiché joue une vidéo avec du son (l'actif d'abord), mini-lecteur ouvert 250 ms plus tard si l'utilisateur n'est pas revenu (`away`). Ne comptent pas comme un départ : les fenêtres posées sur la principale (enfant, popup d'extension `chrome-extension://`), les outils de développement détachés, le mini-lecteur lui-même. Au retour du focus dans la fenêtre principale, ce mini-lecteur (et seulement lui, pas celui ouvert à la main) se ferme et la vidéo continue dans son onglet.
- **À la demande** : entrée « Image dans l'image » du menu contextuel d'une page qui contient une vidéo, action de la barre de commande `pip`, et `openPip`.
- **Revenir à l'onglet** : la fenêtre se ferme, la vue revient dans la fenêtre principale, l'onglet est activé (et son espace), la fenêtre principale passe au premier plan.
- **Fermer** : laisse la vidéo jouer, remet la page en place dans son onglet (sans l'afficher si ce n'est pas l'onglet actif ; le média reste pilotable depuis la carte média), ferme la fenêtre.
- **Seulement une vidéo qui a une image** (`videoWidth` et `videoHeight` non nuls) : un lecteur de musique qui joue le son dans un `<video>` sans image (ex. Tidal) n'ouvre jamais de mini-lecteur, ni automatiquement ni à la demande, et le menu contextuel ne propose pas l'entrée (`hasVideo`). Vérification : étape (7) de `SIGNET_PIPCHECK`.
- Un seul mini-lecteur à la fois : en ouvrir un autre ferme le précédent (vidéo du précédent en pause).
- Si la page navigue ou que l'onglet est fermé : le mini-lecteur se ferme proprement.
- L'image dans l'image native déclenchée par un site lui-même (bouton du lecteur) n'est pas interceptée.

## v14 — confirmation avant de quitter (⌘Q)

Pour éviter de fermer le navigateur par accident. Contrat : `Settings.confirmQuit?` (absent = activé), `QuitHudState`, `onQuitState`. Code : app/desktop/src/main/app/quit.ts, route `#quit` (app/desktop/src/renderer/src/overlay/QuitHud.tsx).

- **Deux façons de quitter** quand le réglage est actif :
  - **maintenir ⌘Q** : une bulle apparaît et sa jauge se remplit en 900 ms ; à la fin, l'app quitte. Relâcher avant la fin arrête la jauge (elle retombe en 150 ms) mais laisse la bulle affichée ;
  - **appuyer deux fois** : un second ⌘Q dans les 2 s qui suivent le premier (bulle affichée) quitte tout de suite.
  - Sans nouvel appui, la bulle disparaît 2 s après le relâchement.
- **Bulle** : petite fenêtre sans cadre, transparente, au premier plan, non focalisable (ne vole pas le focus), centrée horizontalement sur la fenêtre de Signet qui a le focus (ou sur l'écran principal s'il n'y en a pas), au premier tiers de sa hauteur. Contenu : pastille arrondie (rayon 14, fond `--surface` translucide flouté, ombre de couche flottante) avec le texte « Maintenez ⌘Q pour quitter » (et en plus petit « ou appuyez à nouveau »), traduit en/fr, et une jauge fine en bas (accent) animée dans la vue (`requestAnimationFrame`) depuis `start`/`duration` envoyés une seule fois par le main (plafond d'attente à 0,4 tant qu'aucune répétition de touche n'est vue, signalée par un seul message `repeating`) ; au relâchement elle revient vers zéro depuis sa valeur courante. Le seuil des 900 ms reste décidé par le main. Apparition en fondu + légère mise à l'échelle (150 ms), `prefers-reduced-motion` respecté.
- **Détection de l'appui maintenu** : le raccourci du menu ne donne pas les relâchements. Écouter `before-input-event` sur tous les `webContents` de l'app (fenêtre principale, vues d'onglet, overlay, barres flottantes, Little Signet, Réglages, Bibliothèque, Boost, mini-lecteur… via `app.on('web-contents-created')`) : `keyDown` de Q avec ⌘ seul (sans Maj ni Option ni Ctrl) → `preventDefault`, démarrer (ou continuer si `isAutoRepeat`) ; `keyUp` de Q ou de ⌘ → relâché. L'élément « Quitter » du menu garde l'affichage du raccourci ⌘Q mais sans enregistrer l'accélérateur (`registerAccelerator: false`) ; s'il est cliqué à la souris, il quitte directement (pas de confirmation pour un choix explicite dans le menu).
- **Sans fenêtre au premier plan** (aucun `webContents` focalisé, par exemple toutes les fenêtres fermées) : comme aucun `before-input-event` n'arrive, garder un raccourci global de secours ⌘Q actif seulement dans ce cas, qui applique la règle des deux appuis.
- **Quitter par d'autres moyens** (menu du Dock, déconnexion, extinction, `app.quit()` interne) : jamais de confirmation.
- **Réglage** : Réglages › Général, interrupteur « Confirmer avant de quitter (maintenir ⌘Q ou appuyer deux fois) ». Désactivé : ⌘Q quitte immédiatement comme avant.

## v15 — groupes d'onglets

Les onglets ouverts depuis un onglet du jour se rangent avec lui dans un groupe : une carte avec un en-tête, comme un dossier. Contrat : `Item.group?` (onglet seul ou vue partagée), `Space.groups?: Record<id, TabGroup>` (`name`, `hue`, `open`), `DragSource` `{ kind: 'group' }`, `DropTarget.groupId?`, `renameGroup`, `toggleGroup`, `showGroupMenu`. Code : packages/shared/src/tree.ts (utilitaires), `app/desktop/src/main/browser/group-actions.ts` et `model/groups.ts`, app/desktop/src/renderer/src/tabs/groups.ts, `GroupRow` (TabRow.tsx).

- **Modèle** : la liste du jour reste à plat ; un groupe est une suite d'au moins deux éléments consécutifs portant le même `group`. `syncGroups` (à chaque `changed()` et au chargement) garde l'invariant : un membre détaché de sa suite ou un groupe réduit à un élément perd l'étiquette ; chaque groupe vivant a un en-tête (créé au besoin), aucun en-tête sans groupe. Jamais d'étiquette hors des onglets du jour.
- **Création** : lien ouvert dans un nouvel onglet (`target=_blank`, `window.open`, clic milieu, ⌘clic, « Ouvrir dans un nouvel onglet » du menu contextuel) depuis un onglet du jour de l'espace courant (`openTab(url, { openerId })`) : le nouvel onglet va à la fin du groupe de l'onglet qui ouvre, créé au besoin autour de lui. Depuis un épinglé ou un favori : onglet du jour ordinaire, en haut. Titre par défaut : celui de l'onglet qui ouvre (nom choisi, titre de la page, sinon domaine) ; teinte : la première de bleu, vert, violet, orange, rose, turquoise, jaune, rouge que n'utilise aucun autre groupe de l'espace.
- **En-tête** (barre latérale) : carte de dossier teintée ; icône du premier onglet, titre dans la teinte, flèche ; pas de croix (fermer le groupe passe par son menu). Clic : replier / déplier (replié, l'onglet actif du groupe reste visible) ; double-clic : renommer en place ; clic droit : Renommer, Couleur, Replier / Déplier, Dissocier le groupe, Fermer le groupe ; glisser l'en-tête : déplace tout le groupe (onglets du jour de n'importe quel espace, jamais au milieu d'un autre groupe ; pas dans les épinglés ni les favoris).
- **Onglets du groupe** : des onglets comme les autres (fermer le premier ne ferme que lui). Menu : « Retirer du groupe » (il se place juste après le groupe). Glisser-déposer : déposé dans la carte (entre deux onglets, sous le dernier, ou sur la moitié basse de l'en-tête), l'élément rejoint le groupe ; ailleurs, il le quitte.
- **Fermer le groupe** : tous ses onglets rejoignent la pile ⇧⌘T avec un même `batch` (et l'en-tête sur le premier) ; ⇧⌘T les rouvre ensemble, regroupés, avec leur titre et leur couleur. Un onglet fermé seul revient dans son groupe si celui-ci existe encore.
- **Épingler / désépingler** : « Épingler le groupe » (menu du groupe) en fait un dossier à la fin des épinglés : même nom, emoji, teinte et état replié ; ses onglets deviennent épinglés (`pinnedUrl`). « Désépingler » (menu d'un dossier qui contient des onglets) fait l'inverse : ses onglets, sous-dossiers aplatis, deviennent un groupe en haut des onglets du jour, avec le nom, l'emoji, la teinte (une teinte libre si le dossier n'en a pas) et l'état replié du dossier. Glisser-déposer : un groupe déposé dans les épinglés (racine ou dans un dossier) devient un dossier à cet endroit ; un dossier déposé dans les onglets du jour devient un groupe à cet endroit (jamais au milieu d'un autre groupe).
- **Icône** : « Choisir une icône… » dans le menu du groupe (comme pour un onglet et un dossier) ouvre le sélecteur d'emoji (panneau de la barre latérale, ou popover sous l'étiquette quand les onglets sont en haut) ; l'emoji (`TabGroup.icon`) remplace l'icône du premier onglet dans l'en-tête et s'affiche devant le titre de l'étiquette. Même chose pour un dossier (`Item.folder.icon`, à la place de l'icône de dossier, partout où il apparaît). Contrat commun : `IconTarget`, `setIcon(target, emoji | null)`, `onPickIcon` (canal `icon:request`), popover `{ kind: 'icon', target }`.
- **Archivage** : automatique, un groupe part d'un bloc quand aucun de ses onglets n'a servi depuis le délai ; « Archiver » et son annulation gardent groupes et en-têtes.
- **Onglets en haut** : le groupe est un fond teinté qui réunit une étiquette (titre en couleur ; clic : replier / déplier, glisser : tout le groupe, clic droit : menu du groupe ; renommer passe par le champ sous l'étiquette) et ses pastilles ; replié, seule reste la pastille active.
- Vérification : `SIGNET_GROUPCHECK=<dossier>` (profil isolé exigé).

## v16 — signets, raccourcis sur tous les systèmes

### Signets (par espace)

Contrat : `Space.bookmarks?: Bookmark[]` (`{ kind: 'link', id, title, url, favicon? }` ou `{ kind: 'folder', id, name, items }`, dossiers imbriqués), `BookmarkTarget` (`folderId?`, `index`), `Settings.showBookmarksBar?` (absent = affichée), popover `{ kind: 'bookmarks', folderId, from?, tabId }`, renommage `{ kind: 'rename', target: { kind: 'bookmark', id } }`, API `toggleBookmark`, `addBookmark`, `moveBookmark`, `openBookmark`, `renameBookmark`, `showBookmarkMenu`, `showBookmarksBarMenu`, `onRenameBookmark`. Code : app/desktop/src/main/features/bookmarks.ts, app/desktop/src/renderer/src/toolbar/BookmarksBar.tsx.

- **Barre** : 32 px sous la barre d'adresse de chaque panneau (mêmes couleurs, celles du site), absente en mode compact ; la carte web descend d'autant (`BOOKMARK_BAR` dans le layout). Signets de l'espace actif, chacun favicon + titre (180 px au plus) ; un dossier : icône, nom, petite flèche. Ce qui ne tient pas passe dans le menu « » » à droite. Vide : une phrase explique comment en ajouter.
- **Ouvrir** : clic sur un lien = dans l'onglet du panneau ; ⌘clic (Ctrl+clic sous Windows et Linux) ou clic milieu = nouvel onglet en arrière-plan. Clic sur un dossier ou sur « » » : menu au-dessus des pages (overlay), sous-dossiers en sous-menus ouverts au survol. Les popovers de l'overlay marchent désormais dans les deux dispositions d'onglets.
- **Ajouter** : ⌘D (ajoute la page à la fin de la barre, ou retire son signet s'il existe, où qu'il soit rangé) ; étoile à droite de l'adresse (pleine, en couleur d'accent, quand la page est en signet) ; onglet glissé depuis la barre latérale ou la rangée du haut et déposé sur la barre (avant / après un signet, dans un dossier par son milieu) ; menu d'un onglet « Ajouter aux signets » / « Retirer des signets ». Pendant le glisser d'un onglet, les zones de vue partagée commencent sous la barre de signets.
- **Ranger** : glisser un signet pour le déplacer, ou le déposer au milieu d'un dossier ; jamais un dossier dans lui-même.
- **Menus** : clic droit sur un lien (Ouvrir, Ouvrir dans un nouvel onglet, Renommer…, Supprimer le signet, Nouveau dossier), sur un dossier (Tout ouvrir dans de nouveaux onglets, Renommer…, Supprimer le dossier, Nouveau dossier), sur la barre (Nouveau dossier, Masquer la barre de signets). Renommer ouvre un petit champ sous le signet (ou sous « » » s'il est caché).
- **Afficher / masquer** : ⇧⌘B, menu Affichage, Réglages › Général « Barre de signets ».
- **Import** : les favoris de Chrome, Brave, Edge et Safari vont dans les signets de l'espace actif : directement dans la barre si elle est vide, sinon dans un dossier au nom du navigateur à la fin de la barre ; la barre s'affiche. Arc continue de créer des espaces.
- Vérification : `SIGNET_BOOKMARKCHECK=<dossier>` (avec `SIGNET_SEED=1`, profil isolé).

### Raccourcis sur Mac, Windows et Linux

- Menus : `CmdOrCtrl` partout (⌘ sur Mac, Ctrl ailleurs). Quitter : ⌘Q sur Mac, Ctrl+Q ailleurs (confirmation par appui maintenu ou double comprise). Raccourcis qui utilisent Control sur Mac et entreraient en conflit ailleurs : aller à l'espace 1–9 = ⌃1…9 sur Mac, Alt+1…9 ailleurs ; vue partagée = ⌃⇧= sur Mac, Alt+Shift+= ailleurs (Ctrl+Shift+= y est le zoom). L'alias caché ⌘⇧S n'existe que sur Mac.
- Textes : écrits à la façon du Mac dans les catalogues ; un post-traitement i18next (`platformKeys`, packages/shared/src/i18n.ts) les écrit pour Windows et Linux (⇧⌘T → Ctrl+Shift+T, ⌥⌘B → Ctrl+Alt+B, ⇥ → Tab ; deux écritures Mac identiques ailleurs n'en font plus qu'une). Le tableau des raccourcis des Réglages, la barre de commande et les indices (`shortcut()`, app/desktop/src/renderer/src/lib/util.ts) suivent la même règle.
- ⌘D ajoute un signet ; épingler l'onglet passe à ⌥⌘P.

### Menu Affichage : coches

Les bascules du menu Affichage sont des cases cochées (coche à gauche quand l'option est active) : Afficher la barre latérale (désactivée avec les onglets en haut), Mode compact, Onglets en haut, Afficher la barre de signets. Le menu est reconstruit dès que l'un de ces états change, quel que soit le chemin (`Browser.apply`).

## v17 — mots de passe

Chacun choisit son gestionnaire dans Réglages › Mots de passe (`settings.passwordManager`, `signet://settings/passwords`) : **Signet** (défaut), une extension de la boutique (Bitwarden, 1Password, Proton Pass, Dashlane, LastPass ; liste `PASSWORD_EXTENSIONS`) ou **Aucun**. Avec une extension, la ligne indique si elle est installée et propose « Installer » (`Extensions.installPasswordManager`) ; le trousseau de Signet ne remplit plus rien et ne propose plus d'enregistrer. Pas de Trousseau iCloud ni de KeePassXC : leurs extensions passent par la messagerie native, absente d'Electron.

### Trousseau de Signet (`passwords.ts`)

- Fichier `passwords.json` (userData, `AtomicWriter`) : `{ id, origin, username, password, createdAt, updatedAt, lastUsedAt? }`, mot de passe chiffré par `safeStorage` (clé dans le trousseau du système). Origine exacte (`https://hôte[:port]`) : un compte ne sert que sur cette origine. Si le chiffrement est indisponible, rien ne s'enregistre (avertissement dans les Réglages).
- Aucun mot de passe dans `Snapshot` ni `SettingsInfo` : la section des Réglages passe par `passwords:list` (sans mots de passe), `passwords:reveal` / `passwords:copy` (Touch ID si disponible, puis une minute de grâce ; le presse-papiers est vidé au bout de 90 s s'il contient encore le mot de passe), `passwords:save`, `passwords:delete`, `passwords:import`, `passwords:export`.
- Import CSV (Chrome, Safari, Firefox, Bitwarden, 1Password, LastPass, Proton Pass, Dashlane : colonnes reconnues par leur nom) ; les identifiants déjà présents (même origine et même identifiant) sont gardés. Export CSV au format de Chrome, après Touch ID, fichier en 0600.

### Dans les pages (`app/desktop/src/preload/passwords.ts`, script de cadre sur toutes les sessions, privées comprises)

- Toucher un champ de connexion (mot de passe, ou identifiant d'un formulaire qui en a un, ou `autocomplete=username`) affiche sous le champ la liste des comptes du site (shadow root fermé) ; un vrai clic ou Entrée remplit identifiant et mot de passe (setter natif + événements `input` / `change`). Jamais de remplissage au chargement.
- Envoi d'un formulaire, Entrée dans un champ, clic sur un bouton près d'un mot de passe rempli : le script envoie `pw:submit`. L'origine est celle du cadre émetteur vue par le main (`senderFrame.origin`), jamais une valeur de la page, et seuls les onglets sont servis. Un identifiant envoyé seul (connexion en deux étapes) est retenu 5 minutes pour la page du mot de passe.
- La liste ne réagit qu'aux gestes réels (`isTrusted`) : une page qui donne elle-même le focus au champ ne peut pas choisir un compte avec des touches ou des clics simulés.
- Même mot de passe déjà enregistré : `lastUsedAt` seulement. Sinon, si `offerSavePasswords` et que l'origine n'est pas dans `neverSavePasswords`, proposition : overlay en mode `password` (carte en haut à droite de la page : Enregistrer ou Mettre à jour / Jamais pour ce site / Pas maintenant, clic à l'extérieur = plus tard) ; le mot de passe reste dans le main. Little Signet, Peek ou overlay déjà ouvert : boîte de dialogue native. Jamais de proposition dans une fenêtre privée.

### Bouton clé de la barre d'adresse

- Juste avant l'étoile des signets, avec le trousseau de Signet seulement (jamais en fenêtre privée) : `TabInfo.passwordKey`, calculé par le main et jamais écrit sur le disque. `pending` (accent) : un identifiant saisi sur le site de l'onglet n'est pas encore enregistré ; `saved` (neutre) : le trousseau a des comptes pour son origine.
- « Pas maintenant » ou clic à l'extérieur ne jettent plus la proposition : elle reste en mémoire dans le main (le mot de passe ne quitte jamais le main) jusqu'à « Enregistrer » ou « Jamais pour ce site », une navigation vers un autre site, la fermeture de l'onglet ou 30 minutes. Une seule proposition par onglet (la dernière saisie l'emporte).
- Clic sur la clé (`passwords:key`) : en attente, la proposition rouvre sous le bouton (carte avec flèche) ; sinon, carte « Mots de passe enregistrés » (overlay en mode `passwords`) avec les identifiants du site (jamais les mots de passe), « Copier » pour chacun et « Gérer les mots de passe… » (Réglages › Mots de passe).
- Le bouton suit la navigation de l'onglet, l'ajout ou le retrait de comptes (Réglages, import, enregistrement) et le changement de gestionnaire.

## v18 — outils de la page : imprimer, enregistrer, code source, orthographe, lecture, traduction

### Imprimer, enregistrer, code source (`pagetools.ts`)

- **Imprimer…** (⌘P, menu Fichier, clic droit sur la page, barre de commande `print`) : boîte d'impression du système (`webContents.print`), qui propose aussi le PDF sur Mac.
- **Exporter en PDF…** (menu Fichier, `save-pdf`) : `printToPDF` (fonds imprimés, A4, ou Lettre pour les régions qui l'utilisent), boîte d'enregistrement dans Téléchargements, nom tiré du titre.
- **Enregistrer la page sous…** (menu Fichier, clic droit « Enregistrer sous… », `save-page`) : page complète (`.html` + dossier des ressources) ou archive web en un fichier (`.mhtml`), selon l'extension choisie.
- **Afficher le code source** (⌥⌘U, menu Affichage, clic droit, `view-source`) : `view-source:<adresse>` dans un nouvel onglet juste après (dans sa fenêtre pour un onglet privé). Pages web et fichiers seulement.

### Orthographe

- Clic droit sur un mot souligné : jusqu'à 5 suggestions (`replaceMisspelling`), « Aucune suggestion » sinon, puis « Ajouter au dictionnaire » (`addWordToSpellCheckerDictionary`), avant les commandes d'édition.
- Réglages › Général › « Vérifier l'orthographe » (`settings.spellcheck`, absent = activé) : `setSpellCheckerEnabled` sur toutes les sessions. Sur Mac, le correcteur du système choisit la langue ; ailleurs, les langues du correcteur suivent les langues préférées des sites (3 au plus).

### Mode lecture (`reader.ts`, monde isolé 1003)

- À chaque chargement (et 900 ms après une navigation dans la page), `isProbablyReaderable` (Readability de Mozilla, embarquée en texte `?raw`) dit si la page ressemble à un article : `TabInfo.reader = 'available'`. Le même examen lit `<html lang>` pour la traduction.
- Bouton livre dans la barre d'adresse (accent quand la lecture est affichée), ⌥⌘R, menu Affichage, clic droit, barre de commande `reader`. Readability extrait l'article d'une copie du document ; le contenu est nettoyé (ni script, ni gestionnaire `on*`, ni adresse `javascript:`, ni formulaire, ni cadre) puis posé par-dessus la page dans un shadow root fermé : l'adresse et l'état de la page ne changent pas, le défilement de la page est suspendu.
- Barre de la lecture (atténuée jusqu'au survol) : taille du texte (14 à 28 px), Serif / Sans serif, thème (automatique, clair, sépia, sombre), largeur de colonne, Quitter (ou Échap). Les choix sont retenus pour toutes les pages (`settings.reader`, `DEFAULT_READER`) : la vue les envoie au main par appels successifs à `next()`.
- Une navigation ferme la lecture ; une navigation dans la page aussi, puis la page est réexaminée.

### Traduction (`translate.ts`, monde isolé 1004)

- Proposée quand la langue déclarée par la page n'est pas parmi les langues préférées (`TabInfo.foreignLang`) : bouton dans la barre d'adresse. Aussi dans le menu Affichage, le clic droit (« Traduire en français ») et la barre de commande, pour toute page web. Langue cible : la première langue préférée des sites (chinois simplifié ou traditionnel gardé).
- Sur place, sans recharger : les textes sont regroupés par paragraphe (plus proche ancêtre qui n'est pas un élément en ligne), chaque nœud balisé `<a i=n>`, envoyés par lots (6000 caractères) au service de Google (`translate_a/t`, format HTML) depuis le main ; les morceaux traduits reviennent dans les mêmes nœuds texte, dans l'ordre du document. Seule la valeur des nœuds change, jamais la structure (React et consorts restent intacts) ; si la langue déplace les mots, la phrase se lit juste mais un lien peut glisser d'un mot. Le titre de l'onglet est traduit aussi.
- Paragraphes visibles d'abord (`IntersectionObserver`, une hauteur d'écran de marge), puis au fil du défilement et des ajouts de la page (`MutationObserver`). Bouton qui respire pendant la première traduction, accent ensuite ; un second clic rend la page d'origine. Échec réseau : page d'origine et message.
- Le texte de la page part chez Google seulement à la demande (jamais automatiquement).

## v19 — raccourcis modifiables

### Liste des commandes (`app/desktop/src/shared/shortcuts.ts`)

- `SHORTCUTS` : une entrée par commande modifiable (identifiant, groupe des Réglages, clé du libellé, raccourci d'origine au format Electron, variante Mac / autres systèmes, raccourcis supplémentaires d'origine comme ⇧⌘] pour l'onglet suivant). La même liste construit le menu (`menu.ts`, fonction `command(id, élément)`), la section Raccourcis des Réglages et les raccourcis affichés par la barre de commande (`shortcutLabel`) et les bulles d'aide de la barre d'outils.
- Commandes sans raccourci d'origine, à choisir : Traduire la page, Enregistrer la page sous…, Exporter en PDF.
- Non modifiables, montrés en lecture seule : ⌘1…9, ⌃1…9, Réglages (⌘,), Plein écran, Quitter (⌘Q, voir v14). Réservés (refusés pour une commande) : ceux-là et le menu Édition (annuler, rétablir, couper, copier, coller, tout sélectionner), Masquer, Masquer les autres, Placer dans le Dock.

### Réglages (`settings.shortcuts`)

- Seuls les écarts sont enregistrés : accélérateur choisi, `null` pour aucun raccourci ; clé absente = raccourci d'origine (avec ses supplémentaires, perdus dès qu'on en choisit un autre). Le main valide l'ensemble (`cleanOverrides` : commande connue, accélérateur valide avec ⌘, ⌃ ou ⌥ sauf F1…F24, aucun doublon, aucun réservé) et refuse tout s'il est incohérent ; le menu est reconstruit quand les écarts changent (`Browser.apply`).
- Section Raccourcis : clic sur la puce d'une commande → « Tapez le raccourci… » ; la touche suivante devient le raccourci (lettre lue dans la disposition du clavier, AZERTY compris, via `navigator.keyboard.getLayoutMap`). Échap annule, retour arrière retire le raccourci (« Aucun »). Raccourci déjà pris par une commande : « Déjà utilisé par … » avec Annuler / Remplacer (l'autre commande n'a plus de raccourci) ; réservé : « Réservé à … ». Une flèche circulaire rétablit le raccourci d'origine d'une ligne modifiée ; « Tout rétablir » efface tous les écarts.
- Pendant la saisie (`shortcuts:recording`), le menu est reconstruit sans enregistrer aucun raccourci auprès du système et ⌘Q n'est plus intercepté (`pauseQuitShortcut`) : ⌘W ou ⌘Q arrivent à la saisie au lieu de fermer ou quitter. La saisie s'arrête (et tout revient) sur Échap, un clic ailleurs, la perte du focus ou la fermeture de la fenêtre.

## v20 — capture d'écran, espaces en dégradé

### Capture d'écran (`capture.ts`, overlay `capture`, `Capture.tsx`)

- Ouverture : ⇧⌘2 (menu Fichier, modifiable), clic droit sur la page « Capture d'écran… », barre de commande `capture`. Dans la fenêtre principale, l'overlay voile la fenêtre et se pose sur la vue web de l'onglet (`rect`) ; ailleurs (Little Signet, fenêtre privée), la partie visible est proposée à l'enregistrement tout de suite.
- Faire glisser choisit une zone (suivi 1:1, dimensions affichées sous la zone, liseré clair ; le voile est l'ombre portée de la zone). Barre en bas de la page, comme l'outil de capture de macOS : Zone / Partie visible / Page entière, puis Copier (Entrée), Enregistrer…, Annuler (Échap).
- Zone et partie visible : `capturePage` (netteté de l'écran). Page entière : protocole de débogage (`Page.getLayoutMetrics` puis `Page.captureScreenshot` avec `captureBeyondViewport`), plafonnée à 16 384 px de côté.
- Copier : le main rend l'image en PNG (data URL), l'overlay la met dans le presse-papiers (`navigator.clipboard.write`, l'API du presse-papiers du main n'accepte plus d'image) puis affiche « Copiée dans le presse-papiers » et se ferme. Enregistrer : l'overlay se ferme, boîte d'enregistrement dans Téléchargements (« Capture <site> <date> à <heure>.png »).
- Autorisations : les autorisations libres (écriture dans le presse-papiers, plein écran, verrouillage du pointeur) sont accordées avant tout examen d'origine, comme le contrôle des autorisations le faisait déjà ; l'interface, chargée depuis un fichier, n'a pas d'origine web.

### Espaces en dégradé

- `Space.theme` (absent = couleur unie, comme avant) : `{ hues: 2 ou 3 teintes, grain: 0 à 1 }`. `Space.hue` reste la première teinte (accent, sélections). Les brouillons venus de l'interface sont validés (`cleanSpaceDraft`, store.ts).
- Formulaire d'espace : contrôle segmenté Uni / Dégradé et aperçu à côté. Uni : les pastilles d'avant. Dégradé : un clic ajoute une teinte (numérotée dans l'ordre du dégradé ; au-delà de 3, la dernière est remplacée), un second clic la retire ; 2 teintes au moins pour enregistrer ; curseur Grain.
- Fenêtre principale : fond `linear-gradient(155deg, …)` des trois teintes mêlées au fond de base, dose multipliée par `--gk` (2,6 pour un dégradé, 1 pour une couleur unie) et par le réglage de teinte d'Apparence. Grain : texture de bruit (SVG `feTurbulence`) sous le contenu, en `overlay`. `--h2`, `--h3`, `--gk`, `--grain` sont des propriétés enregistrées et animées comme `--h` : un espace uni pose les trois teintes égales, si bien que le passage d'un espace uni à un dégradé glisse en douceur (`useSpaceTheme`).
- Bibliothèque : la pastille de l'espace montre son dégradé.

## v21 — dossiers dynamiques

Un dossier des épinglés qui se remplit seul depuis une source et se met à jour tout seul (`livefolders.ts`, lecture des flux dans `feeds.ts`).

- Sources (`LiveSource`) : un flux RSS 2.0 ou Atom (blog, chaîne YouTube, versions d'un dépôt GitHub…), ou une page ordinaire qui en annonce un (`<link rel="alternate" type="application/rss+xml|atom+xml">`) ; les demandes de fusion GitHub « relectures qui me sont demandées » ou « mes demandes ouvertes » (API de recherche, avec un jeton personnel).
- Modèle : `Item` dossier + `live: { source, refreshedAt?, error?, refreshing?, seen? }` (relu au démarrage sans `refreshing`). Ses enfants sont des onglets épinglés ordinaires, un par adresse (`pinnedUrl`) : un onglet existant garde son état d'une mise à jour à l'autre. Les 15 éléments les plus récents (par date si la source en donne, sinon dans son ordre). Un élément disparu de la source est retiré, sauf si son onglet est affiché (actif ou panneau d'une vue partagée).
- « Nouveau » : les adresses ouvertes sont retenues dans `seen` ; les autres onglets portent un point (`TabInfo.liveUnread`, calculé par le main, non écrit). À la création, tout est déjà « vu ».
- Mise à jour : 8 s après le démarrage, toutes les 15 minutes, et « Actualiser » (menu du dossier). `net.fetch` avec un délai de 15 s, 5 Mo au plus. Un échec garde les éléments précédents et note l'erreur : petite pastille sur l'icône du dossier, message dans sa bulle ; une flèche tourne sur l'icône pendant la mise à jour.
- Création : clic droit sur le bouton « Nouveau dossier » de la barre latérale, menu de l'espace, ou barre de commande « Nouveau dossier dynamique ». Le formulaire (popover de l'overlay, dans les deux dispositions) propose « Flux ou site » (adresse) ou « GitHub » (choix de la requête) ; la source est vérifiée avant la création, le dossier prend le titre du flux. « Modifier la source… » rouvre le même formulaire.
- Un dossier dynamique est géré : pas de dépôt d'onglet dedans, pas de sous-dossier, absent de « Ranger dans un dossier ». Icône de sa source (`Rss`, `GitPullRequest`) sauf emoji choisi.
- Jeton GitHub : Réglages › Général › Dossiers dynamiques. Vérifié auprès de GitHub (`/user`), puis enregistré chiffré par `safeStorage` dans `github-token.bin` (userData, 0600) ; jamais dans state.json ni envoyé à une vue (l'interface ne sait que s'il existe). Lien vers la page de création d'un jeton avec l'accès `repo`.

## v22 — notes et Easels

Des documents qui vivent dans la barre latérale comme des onglets (épinglés, du jour, glissés, archivés, en vue partagée) : `signet://note/<id>` et `signet://easel/<id>` (UUID).

### Ouverture et adresses

- Création : menu Fichier « Nouvelle note » (⌃⌘N, modifiable, Ctrl+Alt+N ailleurs) et « Nouvel Easel » (sans raccourci d'origine), barre de commande `new-note` / `new-easel`, clic droit sur « Nouvel onglet » de la barre latérale (menu Nouvel onglet / Nouvelle note / Nouvel Easel). Le document s'ouvre dans un nouvel onglet de l'espace actif.
- L'onglet garde l'adresse `signet://…` ; il charge l'interface de l'app sur la route `#note/<id>` ou `#easel/<id>` (`resolveInternal` → `noteAppUrl`, et `displayUrl` fait l'inverse). Titre et icône viennent de la page (`document.title`, favicon SVG en data URL) : la barre latérale, l'historique et la barre de commande les reprennent sans code propre.
- Fermer ou archiver l'onglet ne supprime rien : rouvrir l'adresse (historique, archives, barre de commande) retrouve le document. Seul « Supprimer » (dans la page, avec confirmation) efface le fichier et ferme ses onglets.

### Stockage et sécurité (`notes.ts`, `app/desktop/src/preload/notes.ts`)

- Un fichier par document dans `userData/notes/<id>.json` (`AtomicWriter`, écrit à la fermeture de l'app). Note : `{ title, html }`, 2 Mo au plus. Easel : `{ title, items, view }`, 30 Mo au plus (images comprises, réduites à 1600 px de côté en WebP).
- Script de page `signet-notes` (sessions des espaces, pas des fenêtres privées) : il n'agit que dans le cadre principal d'une page de l'interface ouverte sur la route d'un document, et expose une `window.signet` réduite (langue, apparence) et `window.signetNotes` (`load`, `save`, `remove`).
- Le main ne sert les canaux `notes:*` qu'au cadre principal d'un onglet dont l'adresse réelle est l'interface de l'app sur une route de document, et tire l'id de cette adresse (jamais d'un argument) : une page ne touche qu'à son document. Une page web ne peut pas ouvrir `signet://note/…` (garde des adresses internes) et n'a ni `window.signet` ni `window.signetNotes`. Les éléments d'Easel sont revalidés dans le main (types, bornes, couleurs, images en data URL).
- HTML des notes et des textes d'Easel nettoyé à l'affichage et au collage (`notesHtml.ts` : balises de texte, liens http(s) et mailto, images http(s) ou data) ; la politique de sécurité de l'interface interdit en plus tout script en ligne.

### Note (`Note.tsx`)

- Éditeur `contentEditable` en colonne, paragraphes `<p>` ; la première ligne est le titre (style de titre, titre de l'onglet). Barre atténuée jusqu'au survol : Titre, Sous-titre, Gras, Italique, Barré, listes à puces, numérotée, à cocher, Citation, Bloc de code, Lien (⌘K, adresse saisie dans la barre), Supprimer.
- Raccourcis en début de ligne : `# `, `## `, `- `, `1. `, `[] `, `> `, ` ``` `. Liste à cocher : clic sur la case. ⌘ clic sur un lien : nouvel onglet.
- Enregistrement 400 ms après la dernière frappe, et en quittant la page.

### Easel (`Easel.tsx`)

- Tableau infini à grille de points : outils Sélection (V), Texte (T), Crayon (P), Image ; six couleurs (texte et traits) ; zoom − / % / + (le % revient à 100 %).
- Défilement à deux doigts : déplacement ; pincement ou ⌘ + défilement : zoom autour du pointeur ; glisser le fond (outil Sélection) : déplacement. Double-clic sur le fond : nouveau texte ; double-clic sur un texte : le modifier (vide en sortant = supprimé).
- Glisser un élément le déplace ; poignée : taille (proportions gardées pour images et traits, largeur seule pour les textes) ; Retour arrière, ou le bouton « Supprimer l'élément » (icône de la touche Effacer, visible seulement avec un élément choisi, pour ne pas le confondre avec la corbeille du haut qui supprime l'Easel) : supprimer. Outil Texte : un clic crée le texte et y place le curseur (le clic n'a pas d'action par défaut, sinon le focus revenait au fond et le texte vide disparaissait). Coller ou déposer une image (ou du texte) l'ajoute ; « Ajouter une image » ouvre un sélecteur de fichiers.
- Outil de capture : bouton « Easel » à côté de Copier et Enregistrer : la capture ouvre un nouvel Easel (image réduite à 1600 px de côté).
- Titre modifiable en haut à gauche ; enregistrement 600 ms après chaque changement (vue comprise).

## v23 — aperçu des onglets, mods

### Aperçu d'un onglet au survol (`preview.ts`, vue `#preview`, `previewHover.ts`)

- S'arrêter 600 ms sur une ligne d'onglet de la barre latérale (fixe ou flottante) ou sur une puce de la rangée du haut affiche une carte : miniature de la page si elle est connue, titre, domaine, et les pastilles « Lecture » (son en cours) et « En veille » (onglet déchargé). Une carte déjà affichée passe d'un onglet à l'autre sans délai ; quitter les onglets la retire après 140 ms. Pas d'aperçu pendant un appui, un glisser, un renommage, un défilement ni quand la fenêtre perd le focus.
- Placement : à côté de la barre latérale, du côté de la page (à gauche pour une barre à droite), aligné sur la ligne ; sous la puce pour la rangée du haut ; toujours dans la fenêtre. La carte vit dans une petite vue transparente posée par le main au-dessus des pages et des barres flottantes, sous l'overlay ; elle n'est jamais sous le pointeur (elle disparaît dès qu'il quitte les onglets). Entrée : glissement de 6 px depuis la barre + fondu (`--t-fast`), sortie par le même chemin ; mouvement réduit : fondu seul. La vue est chargée d'avance, à la première disposition.
- Miniatures : `capturePage` réduite à 560 px de large (JPEG, en mémoire seulement), prise 900 ms après qu'un onglet apparaît, au survol d'un onglet affiché, et toutes les 15 s pour les onglets affichés quand la fenêtre a le focus. 60 au plus (les plus anciennes partent), oubliées à la fermeture de l'onglet ; jamais pour un onglet de fenêtre privée.
- Réglages › Apparence › « Aperçu des onglets au survol » (`settings.tabPreview`, absent = activé).

### Mods de l'interface (`mods.ts`, Réglages › Mods)

- Un mod = un nom et une feuille CSS, activable à part. Les mods actifs, dans l'ordre de la liste, sont assemblés en une feuille insérée (`insertCSS`) dans chaque vue qui affiche l'interface de l'app (repérée à chaque `dom-ready` par son adresse, `isAppUrl`) : fenêtre principale, barres flottantes, overlay, Réglages, Bibliothèque, Little Signet, fenêtres privées, mini-lecteur, recherche, aperçu… Jamais dans les pages web. Une modification remplace la feuille partout aussitôt (l'interface elle-même sert d'aperçu).
- Fichier `mods.json` (userData, `AtomicWriter`) : `{ disabled, mods: [{ id, name, css, enabled }] }`, 200 000 caractères au plus par mod.
- Section des Réglages : « Désactiver tous les mods », liste (monter, descendre, exporter en `.css`, modifier, supprimer avec confirmation en place, interrupteur), « Importer… » (nom tiré d'un en-tête `/* name: … */` ou du nom du fichier), « Nouveau mod » ; éditeur à chasse fixe (⌘S enregistre, Tab insère deux espaces, Échap annule) ; aide qui cite les points d'accroche (`--h`, `--accent`, `--bg`, `--surface`, `html.route-main`, `.sidebar`, `.toolbar`, `.tabstrip`, `.bm-bar`, `.palette`).
- Recours si un mod rend l'interface inutilisable : menu Affichage › « Mods de l'interface » (case cochée, natif : aucun CSS ne peut le masquer), et lancement avec `--safe-mode` (ou `SIGNET_SAFE_MODE=1`) qui n'applique aucun mod pendant la session (la section le signale).

## v24 — sauvegarde et restauration

Réglages › Sauvegarde (`signet://settings/backup`, backup.ts, BackupSettings.tsx). Un fichier `.signetbackup` : JSON compressé (gzip) `{ format: 'signetbackup', version: 1, createdAt, appVersion, sections }`, écrit en 0600.

- Parties, chacune à cocher (`BACKUP_SECTIONS`) : réglages, favoris, espaces et onglets épinglés (dossiers et dossiers dynamiques compris), onglets du jour, signets, historique, archives, Boosts, autorisations des sites, Mods, notes et Easels, mots de passe, extensions (leur identifiant, réinstallées depuis la boutique au lancement suivant l'import). Jamais : cookies et sessions, cache, liste des téléchargements, jeton GitHub. Les onglets sont sauvegardés par leur contenu (adresse, titre, icône, nom et emoji choisis), pas par leur identifiant ; les onglets du jour et les signets gardent le repère de leur espace (id, nom).
- Mots de passe : décochés par défaut ; les cocher demande un mot de passe de sauvegarde (6 caractères au moins, confirmé) et Touch ID. Ils sont déchiffrés du trousseau puis rechiffrés dans le fichier (scrypt N = 2¹⁵ + AES-256-GCM) ; le reste du fichier est en clair.
- Import : choisir un fichier → résumé (date, version, parties présentes avec leur nombre) → cocher les parties → « Ajouter à l'existant » ou « Remplacer l'existant » → mot de passe de la sauvegarde si les mots de passe sont cochés → confirmation → redémarrage.
  - Ajouter : rien n'est effacé, rien n'est ajouté deux fois (même adresse d'onglet, même signet, même origine et identifiant, même Mod, même id de note ; historique fusionné par adresse). Un espace de même id reçoit les épinglés qui lui manquent.
  - Remplacer : chaque partie cochée remplace la sienne. Les réglages sont toujours remplacés, quel que soit le choix.
  - Onglets du jour et signets vont dans l'espace de même id, sinon de même nom, sinon l'espace actif. Les onglets que plus rien ne référence sont retirés de l'état.
- Application : l'app écrit tout ce qui attend en mémoire, fait une copie de sécurité complète sans les mots de passe (`userData/backups/safety-<date>.signetbackup`, les 3 dernières gardées), modifie les fichiers du profil, puis `app.relaunch()` et `app.exit(0)` (sans les sauvegardes de fermeture, qui réécriraient l'ancien état). Le chargement suivant revalide tout (état, signets, trousseau, notes).
- Copies de sécurité listées dans la section : « Restaurer… » les ouvre comme un fichier choisi (mode Remplacer présélectionné) ; « Afficher le dossier ».

## v25 — sites installés comme apps

À la façon de Chrome (« Installer la page en tant qu'application ») : `webapps.ts`, route `#webapp` (`WebApp.tsx`), overlay `webapp` (`WebAppInstall.tsx`), Réglages › Apps (`WebAppSettings.tsx`).

### Installer

- Entrées : Fichier › « Installer ce site comme app… » (raccourci modifiable `installApp`, aucun par défaut), menu de l'onglet, clic droit sur la page (hors lien, image et champ), barre de commande. Seulement une page http(s) d'un onglet de la fenêtre principale (sinon une boîte l'explique). Si une app installée couvre déjà l'adresse, l'entrée devient « Ouvrir dans l'app « nom » » et la page y passe.
- Inspection (monde isolé 1005) : manifeste (`link rel=manifest`, lu avec la session de l'onglet, 512 Ko au plus), icônes déclarées, `application-name`, `theme-color`. Adresse de départ = `start_url` du manifeste si elle est sur la même origine, sinon la page sans fragment ; portée = `scope` du manifeste (même origine, contenant le départ), sinon l'origine. Nom = `name`, `short_name`, `application-name`, titre de la page, hôte.
- Icône : masquables d'au moins 192 px d'abord, puis les autres (manifeste, `apple-touch-icon`, icônes de la page, favicon de l'onglet, `/favicon.ico`), au-dessus de 512 px la plus petite, en dessous la plus grande ; 4 essais au plus, 5 Mo au plus. Elle arrive à l'overlay en data URL.
- Overlay (carte en haut à droite de la page, comme la proposition de mot de passe) : aperçu de l'icône, nom modifiable, Annuler / Installer. L'icône est composée dans un canevas de 1024 px selon la grille macOS (plaque de 824 px, coins de 185 px, ombre) : remplie par l'icône si elle est masquable ou carrée, opaque et nette (≥ 120 px), sinon posée à 60 % sur fond blanc avec un liseré ; sans icône, l'initiale du nom sur la teinte de l'espace.
- Icône personnalisée : cliquer sur l'icône (pastille ＋ au survol) ou « Choisir une image… », déposer une image sur la carte, ou la coller (⌘V). PNG, JPEG, WebP, GIF, SVG, ICO ou BMP, 15 Mo au plus (sinon « Image illisible »). Elle remplace l'icône du site ; trois cadrages : Remplir (la plaque arrondie entière), Ajuster (à 60 % sur la plaque blanche), Telle quelle (l'image seule sur tout le carré, pour une icône déjà dessinée pour le Mac). Cadrage proposé : Remplir pour un carré opaque d'au moins 120 px, sinon Ajuster. « Icône du site » (ou « Sans image ») revient à l'icône trouvée.
- Installer : l'app est enregistrée (`userData/webapps.json`, icône `userData/webapps/<id>.png`), la page passe dans la fenêtre de l'app (un onglet du jour se ferme, un épinglé reste), puis le lanceur est créé.

### Fenêtre d'une app

- `BrowserWindow` sans onglets : barre de 38 px (route `#webapp`, feux tricolores à `{14, 12}`) puis la page sur tout le reste, sans marge. Taille et position retenues par app ; plusieurs fenêtres d'une même app en cascade de 24 px. Session : celle de l'espace où l'app a été installée s'il est isolé, sinon le profil commun.
- Barre aux couleurs du site (`themeColor` de la page, sinon celle du manifeste) : Précédent, Recharger / Arrêter, icône et titre de la page au centre, Copier l'URL, « Ouvrir dans Signet » (la page devient un onglet du jour, la fenêtre se ferme). Page hors de la portée (connexion, lien externe) : pastille du domaine (cadenas en https, rouge sinon) suivie du titre, et un bouton « Revenir à <app> » qui recharge l'adresse de départ.
- Liens : `target=_blank` et `window.open` dans la portée → nouvelle fenêtre de l'app ; hors portée → onglet de la fenêtre principale ; popups avec `features` (OAuth) → vraies fenêtres. Menu contextuel sans Peek ni vue partagée ; « Ouvrir dans un nouvel onglet » va dans la fenêtre principale.
- Menus : la fenêtre d'app se comporte comme une Little Signet (⌘W la ferme, ⌘R, ⌘[, ⌘], zoom, ⇧⌘C, outils de développement visent sa page ; les commandes d'onglets ramènent la fenêtre principale). Le titre de la fenêtre est celui de la page.
- Fermer la fenêtre oublie l'onglet. Les fenêtres d'app ne sont pas rouvertes au lancement suivant.

### Lanceur

- Bundle `~/Applications/Signet Apps/<nom>.app` (`SIGNET_WEBAPPS_DIR` le remplace pour les vérifications) : `Info.plist` (identifiant `dev.signet.app.webapp.<id>`, `LSUIElement`), `app.icns` (`iconutil`, 16 à 1024 px), script `launcher`. Écrit dans un dossier caché puis renommé ; LaunchServices est prévenu (`lsregister -f`). Un nom déjà pris reçoit « 2 », « 3 »… ; on ne supprime ou remplace jamais un bundle dont l'identifiant n'est pas celui de l'app.
- Version empaquetée : le script fait `open -b dev.signet.app signet://webapp/<id>` (schéma `signet` déclaré dans `build.protocols`) ; développement : il relance Electron avec `--signet-webapp=<id>`, qui passe la main à l'instance ouverte (`second-instance`).
- Lancement reçu (`open-url`, `second-instance`, argv au démarrage) : fenêtre de l'app au premier plan, ou nouvelle fenêtre sur l'adresse de départ. Démarrage à froid par un lanceur : seule la fenêtre de l'app s'ouvre (le Dock ramène la fenêtre principale). App inconnue : fenêtre principale et boîte « Cette app n'est plus installée ».
- Limite : un lanceur ne reste pas ouvert ; dans le Dock et ⌘⇥, la fenêtre de l'app apparaît sous l'icône de Signet.

### Réglages › Apps

Liste (icône, nom, domaine, espace du profil, « lanceur supprimé ») : Ouvrir, Renommer (sur place ; le lanceur est recréé sous le nouveau nom), Afficher dans le Finder (recrée le lanceur s'il a disparu), Désinstaller (confirmation en place ; ferme ses fenêtres, supprime le lanceur et l'icône). La barre de commande propose « Ouvrir l'app « nom » » pour chaque app. Canaux : `webapp:install`, `webapps:list|open|rename|reveal|remove`, `webapp:ready|home|open-in-main|copy-url` ; événements `webapp:state`, `webapps:changed`.

## v26 — outils de développement intégrés

Contrat : `TabInfo.devtools`, `DevToolsDock`, `DevToolsPrefs` (`Settings.devtools`), `Pane.devtools` ; canaux `devtools:dock`, `devtools:close`, `devtools:size`, `devtools:menu`. Module devtools.ts, carte DevToolsCard.tsx.

- Chaque onglet a ses propres outils, dans une `WebContentsView` créée par Signet : `setDevToolsWebContents` puis `openDevTools({ mode: 'detach' })`. Une vue neuve à chaque ouverture (une vue d'outils déjà utilisée ne peut pas resservir) ; fermer les outils ferme sa `webContents`.
- Trois emplacements : **à droite** ou **en bas** de la page (carte à part dans la carte du panneau, séparée de la page par l'écart de 8 px de la vue partagée), ou **dans une fenêtre** (titre « Outils de développement — hôte », position retenue ; barre de titre masquée : la fenêtre charge la route `#devtools`, un en-tête de 36 px avec les feux tricolores, le même titre et les mêmes boutons que la carte intégrée — À droite et En bas rattachent les outils à la page, grisés si l'onglet n'est pas dans la fenêtre principale —, et la vue des outils en dessous). La même vue passe d'un emplacement à l'autre sans rouvrir les outils.
- Disposition : la carte du panneau est découpée avant le calcul de la barre de panneau ; la page garde 320 px de large ou 200 px de haut, les outils 320 px ou 160 px (moitié-moitié si la place manque). Les outils intégrés ont un en-tête de 36 px (icône, « Outils de développement », À droite / En bas / Fenêtre, fermer ; à droite l'en-tête est une zone `drag`), vue à rayon 12 dessous. Vue partagée : chaque panneau peut avoir ses outils. Mode image dans l'image : rien pour la page qui y est.
- Séparateur entre la page et les outils : glisser pour redimensionner (taille en px, appliquée en direct, retenue à la fin du glisser), double-clic pour la taille d'origine (480 px à droite, 320 px en bas).
- Emplacement et tailles : ceux du dernier choix (`Settings.devtools`, à droite par défaut), pour chaque nouvelle ouverture ; l'état ouvert/fermé et l'emplacement restent propres à chaque onglet, en mémoire seulement (rien n'est rouvert au redémarrage).
- Ouvrir : ⌥⌘I (bascule), palette (« Outils de développement », « … à droite », « … en bas », « … dans une fenêtre »), menu contextuel « Inspecter » (ouvre les outils puis y sélectionne l'élément). Les outils prennent le focus à l'ouverture et quand ils changent d'emplacement ; à la fermeture, il revient à la page.
- Barre du panneau : bouton `</>` (accent) tant que les outils de l'onglet sont ouverts, quel que soit leur emplacement ; clic → menu natif (À droite, En bas, Dans une fenêtre à part, Fermer les outils).
- Little Signet, fenêtre d'app, fenêtre privée, Peek : outils toujours dans une fenêtre. Un onglet parti dans une autre fenêtre avec ses outils intégrés : ⌥⌘I les montre dans une fenêtre au lieu de les fermer.
- Fenêtre des outils : les commandes du menu visent la page inspectée (⌘R recharge la page, ⌥⌘I ferme les outils, ⌘W ferme la fenêtre et les outils).
- Un lien ouvert depuis les outils (`devtools-open-url`) ouvre un onglet. Un onglet dont les outils sont ouverts n'est jamais déchargé automatiquement.
- Captures de développement : `SIGNET_DEVTOOLS=right|bottom|window` ouvre les outils de l'onglet actif ; leur vue est composée dans la capture.

## v27 — mode développeur

Contrat : `Settings.devMode`, `Settings.devKeepOutOfHistory`, `TabInfo.consoleErrors`, `TabInfo.consoleWarnings` ; canal `devtools:console` ; raccourci `jsConsole` (⌥⌘J). Modules devmode.ts et app/desktop/src/shared/devhosts.ts ; section Réglages › Développement (`signet://settings/developer`).

- Pages locales (`isLocalDevUrl`) : http(s) sur `localhost`, `*.localhost`, `*.local`, `*.test`, `127.x.x.x`, `::1`, `0.0.0.0`, et les adresses du réseau local (`10.x`, `192.168.x`, `172.16` à `172.31`).
- Réglages › Développement : interrupteur « Mode développeur » (absent = coupé ; il affiche aussi le menu Développement, voir v28) ; quand il est actif, « Garder les pages locales hors de l'historique » (absent = enregistrées) ; rappel des raccourcis des outils (⌥⌘I) et de la console (⌥⌘J), tels qu'ils sont réglés.
- Barre du panneau, mode actif et page locale : le port suit l'hôte, en couleur secondaire (`localhost:5173 / titre`). Compteur de la console : pastille rouge pleine (nombre d'erreurs) et pastille ambre (avertissements), seulement si l'un des deux n'est pas nul, 99+ au-delà ; bulle « 3 erreurs · 1 avertissement · Ouvrir la console » ; clic → outils ouverts sur la console. Affiché aussi en barre étroite.
- Comptage : messages `console-message` de niveau erreur ou avertissement (exceptions non rattrapées comprises), plus les ressources en échec (`webRequest.onCompleted` avec un statut ≥ 400, `onErrorOccurred` sauf `ERR_ABORTED` et `ERR_BLOCKED_BY_CLIENT`), comme la console de Chromium. Remise à zéro à chaque nouvelle page (navigation du cadre principal qui change de document) ; les navigations internes d'une app monopage gardent le compte. Diffusion regroupée toutes les 150 ms au plus. Couper le mode efface les compteurs ; rien n'est compté avant qu'il soit actif.
- Historique : avec l'option, une page locale n'est pas enregistrée (ni donc proposée par la barre de commande) ; les onglets ouverts restent cherchables.
- Console JavaScript (⌥⌘J, menu Affichage, palette, compteur) : ouvre les outils de l'onglet à leur emplacement habituel (ou les montre s'ils sont ouverts) et affiche le panneau Console. Les outils rétablissent leur dernier panneau en se chargeant : le panneau demandé est affiché après `devtools-opened`.

## v28 — menu Développement

Contrat : `DevOverrides`, `DevNetwork`, `DevUserAgent`, `TabInfo.devOverrides` ; canal `develop:menu`. Module develop.ts.

- Menu **Développement** de l'app (entre Navigation et Fenêtre, comme dans Safari), seulement avec le mode développeur : Outils de développement, Console JavaScript ; Vider le cache et recharger (cache de toute la session de l'onglet, puis rechargement sans cache) ; Effacer les données du site et recharger (cookies et stockage de l'origine de la page) ; puis les réglages de l'onglet : Désactiver le cache, Désactiver JavaScript, Thème de la page (Suivre le système / Forcer le clair / Forcer le sombre), Réduire les animations, Connexion (Sans limitation / 4G rapide / 4G lente / 3G / Hors ligne), Agent utilisateur (Par défaut / Safari Mac, iPhone, iPad / Chrome Android / Firefox Mac) ; Réinitialiser les réglages de l'onglet. Les commandes visent l'onglet de la fenêtre au premier plan ; les coches suivent l'onglet actif de la fenêtre principale (le menu est reconstruit quand il change ou que ses réglages changent).
- Réglages de l'onglet : propres à l'onglet, tant que sa vue existe (fermer ou décharger l'onglet les efface), jamais enregistrés. Appliqués par le protocole de débogage déjà attaché pour les langues (languages.ts) : `Network.setCacheDisabled`, `Emulation.setScriptExecutionDisabled`, `Emulation.setEmulatedMedia` (`prefers-color-scheme`, `prefers-reduced-motion`), `Network.emulateNetworkConditionsByRule` (motif vide = toutes les requêtes ; `Network.emulateNetworkConditions` en secours), `Emulation.setUserAgentOverride` (toujours avec les langues, réappliqué après un changement des langues ; `navigator.platform` assorti). Le domaine Network n'est activé que pendant qu'un réglage de cache ou de connexion est actif. Désactiver JavaScript ou changer d'agent utilisateur recharge la page.
- Connexions : valeurs des préréglages de Chromium — 4G rapide 165 ms, 9 Mb/s ↓, 1,5 Mb/s ↑ ; 4G lente 562,5 ms, 1,6 Mb/s ↓, 750 kb/s ↑ (débits × 0,9) ; 3G 2 s, 500 kb/s (× 0,8) ; hors ligne. Agents utilisateurs : Safari 26 (Mac, iPhone, iPad en version mobile), Chrome Android et Firefox Mac à la version de Chromium du moment (+ 3 pour Firefox).
- Barre du panneau : bouton fiole (accent) tant qu'au moins un réglage est actif sur l'onglet ; bulle « Réglages de développement actifs : JavaScript désactivé, 3G » ; clic → menu natif des réglages de l'onglet (sans Vider le cache ni Effacer les données).
- Barre de commande, mode développeur : les mêmes commandes pour l'onglet actif, formulées selon l'état (« Désactiver JavaScript dans l'onglet » / « Réactiver… », « Simuler une connexion : 3G », « Agent utilisateur : Safari — iPhone »…), icône fiole.
- Captures de développement : `SIGNET_DEVELOP=<JSON DevOverrides>` applique des réglages à l'onglet actif et affiche ce que la page en voit (`window.__o`) ; `SIGNET_NETTIME=1` chronomètre le chargement d'une adresse neuve.

## v29 — mode responsive

Contrat : `DevDevice`, `DevResponsive`, `DevOverrides.responsive`, `Pane.responsive`, `Settings.responsive` ; canal `develop:responsive` ; raccourci `responsive` (⇧⌘M, menu Développement). Liste des appareils : app/desktop/src/shared/devices.ts ; interface : Responsive.tsx.

- Appareils (px CSS en portrait, densité, mobile) : iPhone SE 375×667 ×2, iPhone 16 393×852 ×3, iPhone 16 Pro Max 440×956 ×3, Pixel 9 412×923 ×2,625, Galaxy S24 360×780 ×3, iPad mini 744×1133 ×2, iPad Pro 11" 834×1210 ×2 (tous mobiles, avec l'agent utilisateur Safari iPhone/iPad ou Chrome Android) ; Ordinateur portable 1280×800 ×2, Écran de bureau 1920×1080 ×1 ; Taille libre (non mobile, 200 à 3840 px).
- Entrer/sortir : ⇧⌘M, case « Mode responsive » du menu Développement et du menu de la fiole, palette. À l'entrée : le dernier appareil utilisé (`Settings.responsive`, iPhone 16 au départ). Propre à l'onglet, comme les autres réglages de développement (la fiole le rappelle : « mode responsive (iPhone 16) »).
- Disposition (main, `Develop.place`) : dans la zone de la page du panneau (sous la barre et les signets), une barre de 40 px en haut, puis la vue web à la taille de l'appareil, réduite si la place manque (échelle ≤ 1, marge de 16 px sur les côtés et en bas), centrée horizontalement, coins arrondis. Autour, la zone prend `--surface-2`. `Pane.rect` est la vue web ; recherche dans la page et captures s'y placent.
- Émulation (protocole de débogage) : `Emulation.setDeviceMetricsOverride` (taille, densité, mobile, échelle d'affichage, écran, orientation portrait/paysage pour un mobile), `Emulation.setTouchEmulationEnabled` et `setEmitTouchEventsForMouse` pour un mobile. Renvoyée seulement quand l'appareil, la taille ou l'échelle change ; effacée à la sortie. La page est rechargée quand l'agent utilisateur ou le caractère mobile change (le tactile et l'agent ne sont lus qu'au chargement). Sans balise `viewport`, une page mobile est mise en page sur 980 px, comme sur un vrai téléphone.
- Barre (renderer) : liste des appareils (Téléphones, Tablettes, Ordinateurs, Taille libre), largeur × hauteur modifiables (Entrée ou sortie du champ ; Échap annule ; passe en Taille libre), Pivoter (échange largeur et hauteur), échelle affichée si < 100 %, Quitter.
- Poignées dans la marge : à droite (largeur, des deux côtés puisque la page est centrée), en bas (hauteur), dans le coin (les deux) ; le glisser passe en Taille libre, envoyé à chaque image.
- Captures de développement : la capture d'une vue en mode responsive couvre la taille émulée, la page réduite en haut à gauche ; la composition la recadre à la taille de la vue.

## v30 — visionneuse JSON

Contrat : `Settings.jsonViewer` (absent = activée). Modules jsonview.ts (main) et app/desktop/src/main/devtools/pages/jsonview.js (script de page embarqué comme texte, `?raw`, lint avec les globales du navigateur).

- Quand : à la fin du chargement d'un onglet (`did-finish-load`, pages http(s) et file), si `document.contentType` est `application/json`, `application/…+json` ou `text/json`, que le document est le `<pre>` que Chromium affiche pour une réponse JSON, que le texte fait 20 Mo au plus et qu'il se lit avec `JSON.parse`. Sinon la page reste telle quelle (JSON invalide compris).
- Où : monde isolé 1006 (la page ne voit ni le script ni l'interface) ; interface dans une racine fantôme fermée posée par-dessus la page ; style par une feuille construite (`adoptedStyleSheets`), qui passe même avec une CSP stricte (`default-src 'none'`, API de GitHub). Couleurs claires ou sombres selon le thème de la page.
- Barre : Arbre / Brut (le texte reçu) / Formaté (indenté de 2 espaces) ; en vue arbre, Tout déplier, Tout replier, champ de recherche et nombre de résultats ; à droite, Copier le JSON (formaté).
- Arbre : clés, chaînes, nombres, booléens et null en couleurs ; chaînes http(s) en liens ; conteneurs repliés résumés (« {…} 3 clés », « […] 12 éléments ») ; conteneur vide affiché `[]` ou `{}` sans flèche. Clic sur la flèche, double-clic ou ⌥-clic sur la ligne : plier/déplier. Document de 3 000 nœuds au plus : tout déplié ; au-delà, la racine et le premier niveau. Les grands conteneurs s'affichent par tranches de 500 enfants (« Afficher les suivants (n restants) »).
- Sélection : clic sur une ligne → barre du bas avec le chemin (`$.items[3].label`, `$["clé spéciale"]`), Copier le chemin, Copier la valeur (JSON indenté pour un conteneur, texte brut pour une chaîne).
- Recherche (150 ms après la frappe) : clés et valeurs qui contiennent le texte, sans tenir compte de la casse, 1 000 résultats au plus ; lignes trouvées surlignées, parents dépliés et tranches étendues jusqu'au résultat ; Entrée / ⇧Entrée : suivant / précédent (ligne courante plus marquée, centrée). Le retour à la vue arbre ramène au résultat courant.
- Copie : commande « copier » sur une zone de texte temporaire (pas de demande d'accès au presse-papiers), « Copié » pendant 1,2 s.
- Réglages › Développement : interrupteur « Visionneuse JSON » (indépendant du mode développeur).
- Captures de développement : `SIGNET_JSONTEST=<script>` s'exécute dans le monde de la visionneuse (`window.__signetJson` : sa racine fantôme) et affiche le résultat.

## v31 — extensions non empaquetées

Contrat : `Settings.unpackedExtensions` (dossiers), `ExtensionInfo.unpacked` ; canaux `extensions:load-unpacked`, `extensions:reload-unpacked`, `extensions:remove-unpacked`. Module unpacked.ts.

- Réglages › Extensions, avec le mode développeur (comme dans Chrome, pour qu'on ne fasse pas charger n'importe quoi à un utilisateur) : ligne « Extension non empaquetée » et bouton « Charger un dossier… » (choix d'un dossier). Sans le mode : une note renvoie à Réglages › Développement. Les extensions déjà chargées restent chargées et listées même si le mode est coupé.
- Chargement : le dossier doit contenir `manifest.json` (sinon alerte « Le dossier … ne contient pas de manifest.json ») ; `loadExtension(dossier, { allowFileAccess: true })` dans la session par défaut, répercuté sur les autres sessions comme une extension de la boutique (et dans une session ouverte plus tard). Un échec (manifeste invalide…) ouvre une alerte avec le message de Chromium. Le dossier est retenu et rechargé à chaque lancement, après les extensions de la boutique.
- Rechargement automatique : le dossier est surveillé (`fs.watch` récursif) ; 400 ms après le dernier changement, l'extension est retirée puis rechargée (même identifiant, tiré du chemin). `.git`, `node_modules`, `.DS_Store` et fichiers temporaires des éditeurs sont ignorés. Après un échec, le prochain changement réessaie. Comme dans Chrome, les scripts de contenu des pages déjà ouvertes ne changent qu'au rechargement de la page.
- Liste : « Non empaquetée » (et « rechargée à HH:MM:SS » après un rechargement), le dossier sur une ligne tronquée par le début (chemin complet en bulle, `~` pour le dossier personnel), l'erreur en rouge si le chargement a échoué (l'extension est alors listée sans identifiant) ; boutons Recharger et Retirer (« La retirer ? Son dossier reste sur le disque. »).
- Retrait : l'extension est déchargée de toutes les sessions et oubliée ; son dossier n'est jamais effacé (le retrait de la boutique, qui efface le dossier installé, n'est pas utilisé). Même chose depuis « Retirer » du menu d'un bouton d'extension.
- Captures de développement : `SIGNET_UNPACKED=<dossier>` charge l'extension, modifie `content.js` (v1 → v2), vérifie le rechargement sur la page active, puis la retire (`SIGNET_UNPACKED_KEEP=1` : la garde).

## v32 — noms locaux

Contrat : `DevAlias`, `Settings.devAliases`. Fonctions partagées dans app/desktop/src/shared/devhosts.ts : `isAliasName`, `aliasTarget`, `resolveAlias`, `aliasFor`.

- Réglages › Développement, « Noms locaux » (indépendant du mode développeur) : liste `nom → adresse` avec Retirer, et une ligne d'ajout (nom, adresse, Ajouter ; Entrée valide). Erreurs en place : nom invalide, nom déjà pris, adresse invalide.
- Nom : minuscules, chiffres et tirets (63 caractères au plus, ni tiret au début ni à la fin), éventuellement suivi de `.test` ou `.localhost` (noms réservés aux tests) ; jamais un vrai domaine (`.com`…), pour ne masquer aucun site et ne rien faire passer pour lui.
- Adresse : seulement une adresse locale (`isLocalDevUrl`) ; `3000` → `http://localhost:3000`, `localhost:5173` et `127.0.0.1:3000/app` complétés en `http://` ; ni requête ni fragment, pas de `/` final. Le main revalide (`settings.update`) : entrées invalides écartées, un seul par nom (le premier).
- Saisie (barre de commande, champ d'adresse) : `nom`, `nom/chemin`, `nom?requête` ou `http(s)://nom/…` ouvre la cible suivie du reste, avant toute autre interprétation (`monsite.test` serait sinon pris pour un domaine en https). La barre de commande propose aussi les noms qui commencent par la saisie (« Nom local → adresse »).
- Barre du panneau : une page sous la cible d'un nom (même origine, chemin égal ou en dessous) affiche le nom à la place de l'hôte et du port (« monsite / titre ») ; l'adresse réelle en bulle. L'historique et le champ d'adresse gardent l'adresse réelle.

## Linux et Windows

Signet est pensé pour macOS mais doit fonctionner sous Linux et Windows (app/desktop/src/main/app/platform.ts).

- Fenêtres : `windowChrome()` — sur Mac, barre de titre masquée, feux tricolores placés, fond translucide (`vibrancy`) ; ailleurs, le cadre et les boutons du système, barre de menus cachée (Alt l'affiche), fond opaque dans le thème. Les API réservées au Mac (`setWindowButtonVisibility`, `setWindowButtonPosition`, `app.dock`, Touch ID, `defaults`) sont protégées par `IS_MAC`.
- Translucidité (Réglages › Apparence) : sans effet hors Mac, les fonds restent opaques (`appearance.ts`).
- Sites installés comme apps : seulement sur Mac (le lanceur est une app macOS) ; les entrées « Installer comme app » sont masquées ailleurs.
- Fermeture : hors Mac (pas de Dock pour rouvrir l'app), fermer la dernière fenêtre visible ou réduite quitte l'app (index.ts) ; sur Mac, l'app reste ouverte comme d'habitude. Sans cela, l'app tournait sans fenêtre et la relancer, même dans une version plus récente, ne faisait que réveiller l'ancienne.
- Wayland (Linux) : l'app tourne sous XWayland (`--ozone-platform=x11`), car Wayland interdit de placer une fenêtre et de la garder au premier plan : le mini-lecteur (position, glisser, redimensionnement, toujours devant) ne marcherait pas. L'option doit être là au démarrage du processus (Electron choisit la plateforme avant notre code) : à l'empaquetage Linux, `scripts/after-pack.mjs` renomme l'exécutable en `signet-bin` et met à sa place un script qui l'ajoute quand `DISPLAY` existe sous une session Wayland. `--ozone-platform=…` passé à la main garde la main. Pas de relance depuis l'app : Chromium lance le nouveau processus sans droit d'élévation (`no_new_privs`) et le montage FUSE de l'AppImage échoue (« Cannot mount AppImage »).
- AppImage (app/desktop/src/main/app/linux.ts) : à chaque lancement, raccourci `~/.local/share/applications/signet.desktop` (chemin de l'AppImage à jour, types web pour le navigateur par défaut, ligne `X-Signet-AppImage=true` ; un raccourci sans elle n'est jamais remplacé) et icône `~/.local/share/icons/hicolor/512x512/apps/signet.png` (variante choisie), désignée par son chemin complet dans `Icon=` (certains docks ne relisent pas le thème d'icônes après leur démarrage). Les fenêtres reçoivent aussi l'icône choisie.
- Import (Réglages › Import, barre de commande) : seulement les navigateurs du système — Arc sur Mac et Windows, Safari sur Mac, Chrome, Chromium, Brave et Edge partout (paquet du système, Flatpak ou Snap sous Linux). Fenêtre de choix des données (espaces, favoris, mots de passe, extensions, historique ; tout coché), déchiffrement des mots de passe propre à chaque système (Trousseau, trousseau GNOME, DPAPI) : voir app/desktop/src/main/features/importers/FORMATS.md. La note sur l'accès complet au disque n'apparaît que sur Mac.
- Menu de l'app : hors Mac, la barre de menus est cachée (Alt l'affiche) ; un bouton ⋮ au bout de la barre d'outils ouvre tout le menu de l'app sous le pointeur (`showAppMenu`).
- Vues d'interface vides (guard.ts `healUiView`) : une vue d'interface dont le preload échoue (`preload-error`) ou dont la page ne charge pas (`did-fail-load`) est rechargée, 1 s, 2 s, 4 s… 30 s au plus ; la page elle-même se recharge si `window.signet` manque (main.tsx). Cas typique : `out/` réécrit par une compilation pendant le lancement. Une ligne de journal par panne (pas par essai). En développement, un refus de connexion du serveur de l'interface (`ERR_CONNECTION_REFUSED` sur `ELECTRON_RENDERER_URL`) veut dire que la session de dev est finie (Ctrl+C) : l'app quitte au lieu de réessayer.
- Menus déroulants (`<select>`) : hors Mac, la liste prend le fond de l'élément, translucide chez nous ; les `option` ont un fond opaque (`--win-bg`).
- Paquets : `npm run dist` (Mac, .dmg), `dist:linux` (AppImage x64), `dist:win` (installateur NSIS x64). Sur un Mac Apple Silicon sans Rosetta, l'AppImage se construit dans Docker (`electronuserland/builder:wine`) ; l'installateur Windows, lui, demande Wine, qui plante sous émulation : il est construit par GitHub Actions (`.github/workflows/build.yml`, lancement à la main ou tag `v…`, paquets des trois systèmes dans les artefacts).

## Hors périmètre

Synchronisation, plusieurs fenêtres principales, recherche dans la page des Little Signet, toute fonctionnalité d'IA.

## Sorties animées des surfaces

Palette, panneau du site, popovers (overlay), panneaux de la barre latérale (téléchargements, formulaires d'espace et d'icône) se matérialisent (échelle 0,96 → 1 + opacité, depuis leur ancre) et repartent par le même chemin en `--t-fast` (120 ms ; simple fondu en mouvement réduit). Tous les chemins de fermeture passent par `closeOverlay` : le main garde la vue de l'overlay posée 120 ms (`OVERLAY_EXIT_MS`) puis la retire ; une nouvelle ouverture annule la sortie. Peek et zones de dépôt se ferment sans animation. L'exécution d'une action de la palette n'attend pas la sortie (elle part immédiatement ; seule la vue s'efface).
