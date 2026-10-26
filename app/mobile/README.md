# Signet mobile

L'app mobile de Signet (iOS et Android), en React Native avec Expo (SDK 58) et `expo-router`. Elle tourne dans **Expo Go** pour le développement, et s'installe aussi compilée sur un iPhone (voir plus bas).

Les **fondations** sont posées : navigation, système de design, store avec sauvegarde, traductions. Chaque fonction (navigateur, barre de commande, onglets, espaces, bibliothèque, réglages, accueil) a son dossier et ses routes ; plusieurs sont encore des écrans provisoires (`StubScreen`).

## Disposition

```
app/mobile/
  app/                 ROUTES (expo-router). Fichiers minces : ils réexportent un écran de src/features/.
    _layout.tsx        racine : gestes, zones sûres, thème, lecture du stockage, pile principale, toasts
    index.tsx          /                      navigateur
    tabs.tsx           /tabs                  sélecteur d'onglets (modale plein écran)
    command.tsx        /command               barre de commande (modale transparente)
    onboarding.tsx     /onboarding            premier lancement (seule route tant que onboardingDone est faux)
    library/           /library, /library/history, /library/bookmarks   (modale, pile propre)
    settings/          /settings, /settings/search-engine|homepage|data|about   (modale, pile propre)
    spaces/edit/[id]   /spaces/edit/<id|new>  éditeur d'espace (feuille)
  src/
    features/<fonction>/   les écrans et composants d'une fonction : browser, command, tabs, spaces, library, settings, onboarding
                           (chaque dossier a un index.ts qui dit ce qui s'y construit)
    ui/                    le système de design (thème, texte, composants) : `import { Button } from '@/ui'`
    state/                 le store (zustand), ses tranches, sa sauvegarde : `import { useStore } from '@/state'`
    lib/                   utilitaires sans écran : i18n (`useT`), haptics
```

L'alias `@/` pointe sur `src/` (tsconfig `paths`, pris en charge par Metro). Les imports entre fonctions passent par leur `index.ts`.

**Qui touche quoi.** Chaque fonction vit dans son dossier de `src/features/` et dans ses routes d'`app/`. Pour éviter les conflits entre personnes qui travaillent en parallèle : `src/ui/`, `src/state/types.ts`, `app/_layout.tsx` et les catalogues `mobile.json` sont des fichiers partagés, on y ajoute sans réorganiser (une tranche du store = un fichier dans `src/state/slices/`).

## Ce qui vient du paquet partagé

La logique qui n'est pas propre au téléphone vit dans `@signet/shared` et a ses tests Vitest. Ici, il reste l'affichage et ce qui touche la plateforme (WebView, stockage, haptique, langue de l'appareil).

| Import                                  | Contenu                                                                                                                        |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `@signet/shared/mobile/workspace`       | Espaces et onglets : `Workspace`, `openTab`, `closeTab`, `moveTab`, `pinTab`, `addSpace`, navigation privée… (fonctions pures) |
| `@signet/shared/mobile/history`         | Historique : `addVisit`, `searchHistory` (frecency), `groupByDay`, `topSites`                                                  |
| `@signet/shared/mobile/bookmarks`       | Signets (arbre de liens et dossiers) : ajouter, déplacer, chercher                                                             |
| `@signet/shared/mobile/settings`        | Type `MobileSettings`, valeurs par défaut, `sanitizeSettings`                                                                  |
| `@signet/shared/mobile/search`          | Moteurs de recherche, `resolveInput(saisie, moteur)`                                                                           |
| `@signet/shared/mobile/suggestions`     | `suggest()` : suggestions de la barre de commande                                                                              |
| `@signet/shared/mobile/color`           | Couleurs d'un espace (accent, fonds teintés, dégradé), calcul porté du bureau                                                  |
| `@signet/shared/mobile/persist`         | Document enregistré : version, migrations, lecture tolérante                                                                   |
| `@signet/shared/{types,urls,site,i18n}` | Types communs (`TabInfo`, `Space`, `Bookmark`…), adresses, domaine d'un site, `createI18n`                                     |
| `@signet/shared/locales/*/mobile.json`  | Les textes de l'app, en anglais et en français                                                                                 |

## Le store

Un seul store zustand (`useStore`), découpé en tranches qui sont de fines enveloppes autour des fonctions pures du paquet partagé. Lire : `useStore((s) => s.settings.theme)` ou un hook de `src/state/hooks.ts` (`useCurrentTab`, `useVisibleTabs`, `useActiveSpace`, `useSpaces`, `usePrivateMode`, `useSettings`, `useIsBookmarked(url)`). Écrire : appeler une action, `useStore.getState().openTab({ url })` ou `const openTab = useStore((s) => s.openTab)`.

| Tranche     | État                            | Actions principales                                                                                                                                                                                                                                      |
| ----------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace` | `workspace`, `lastClosed`       | `openTab`, `closeTab`, `undoCloseTab`, `activateTab`, `updateTab`, `moveTab`, `pinTab`, `favoriteTab`, `closeTodayTabs`, `setPrivateMode`, `closePrivateTabs`, `setActiveSpace`, `addSpace`, `editSpace`, `removeSpace`, `moveSpace`, `reportNavigation` |
| `history`   | `history`                       | `addVisit`, `removeHistoryEntry`, `clearHistory(since?)`                                                                                                                                                                                                 |
| `bookmarks` | `bookmarks`                     | `addBookmark`, `addBookmarkFolder`, `removeBookmark`, `removeBookmarkByUrl`, `updateBookmark`, `moveBookmark`, `toggleBookmark`                                                                                                                          |
| `settings`  | `settings`                      | `setSettings(patch)`, `resetSettings`                                                                                                                                                                                                                    |
| `ui`        | `ui.onboardingDone`, `hydrated` | `setOnboardingDone`                                                                                                                                                                                                                                      |

Règles à connaître :

- **Les onglets privés** vivent dans `workspace.priv`, jamais enregistrés, jamais dans l'historique. `reportNavigation(tabId, page, patch)` est la porte d'entrée des visites : elle met l'onglet à jour et n'ajoute à l'historique que hors navigation privée et page chargée.
- **Sauvegarde** : `state/persistence.ts` lit AsyncStorage au lancement (`hydrated` passe à vrai, la racine n'affiche que le fond avant) puis écrit 600 ms après le dernier changement, et tout de suite quand l'app passe en arrière-plan. Un seul document JSON versionné (`signet.mobile.state`) ; une lecture illisible donne les valeurs par défaut, jamais une erreur. L'ancien format du squelette (`signet.tabs.v1`) est migré une fois.
- **Langue** : `settings.language` est appliquée par `persistence.ts`. Dans un composant, `const t = useT()`.

## Le système de design (`src/ui/`)

Tout s'importe de `@/ui`. Le thème (`useTheme()`, `useColors()`) suit **la teinte de l'espace actif** (même calcul que le bureau, en OKLCH) et le réglage clair / sombre / auto ; la navigation privée passe en sombre violet-gris.

| Composant                               | Rôle et props principales                                                                                                                                                                     |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Text`                                  | `variant` (largeTitle, title1-3, headline, body, callout, subhead, footnote, caption, caption2), `color` (text, secondary, tertiary, accent, danger, onAccent, ou couleur), `weight`, `align` |
| `GlassBar`                              | Barre de verre (flou iOS, voile teinté ; opaque sous Android ou « Réduire la transparence ») : `edge` (top, bottom, none), `floating`, `intensity`                                            |
| `IconButton`                            | `icon`, `label` (obligatoire), `onPress`, `onLongPress`, `size`, `tone`, `filled`, `badge`, `disabled` ; zone 44 pt, haptique légère                                                          |
| `Button`                                | `title`, `onPress`, `variant` (primary, secondary, plain, destructive), `size` (md, lg), `icon`, `loading`, `fullWidth`                                                                       |
| `ListSection` + `ListRow` + `SwitchRow` | Listes style Réglages iOS. Row : `title`, `subtitle`, `icon`, `iconColor`, `left`, `value`, `right`, `selected`, `chevron`, `destructive`, `onPress`                                          |
| `Screen`, `Sheet`                       | `Screen` : fond, zones sûres (`edges`), `scroll`, `padded`. `Sheet` : contenu d'une feuille (`title`, `doneLabel`, `onDone`)                                                                  |
| `TextField`, `SearchField`              | Champs : `label`, `error`, `right` ; `SearchField` : `value`, `onChangeText`, `clearLabel`. Hauteur fixe : **jamais `flex: 1` sur un champ dans une colonne**                                 |
| `Favicon`                               | `uri`, `url`, `size` ; lettre sur fond teinté en repli                                                                                                                                        |
| `EmptyState`                            | `icon`, `title`, `message`, `action`                                                                                                                                                          |
| `toast` + `ToastHost`                   | `toast.show({ message, actionLabel, onAction, duration })` depuis n'importe où ; l'hôte est monté dans `app/_layout.tsx`                                                                      |
| `SegmentedControl`                      | `options` (`value`, `label`, `icon`), `value`, `onChange`                                                                                                                                     |
| `PressableScale`                        | Base de tout ce qui se presse : ressort, échelle 0,96, mouvement réduit respecté ; `haptic`                                                                                                   |
| `icons`                                 | Les icônes lucide utilisées (`icons.Search`). Pour en ajouter, une ligne dans `src/ui/icons.ts` (import par icône : le paquet entier est trop lourd)                                          |
| `useStackOptions`, `HeaderDone`         | En-têtes de pile aux couleurs du thème ; bouton OK d'une modale                                                                                                                               |
| `StubScreen`                            | Écran provisoire des routes pas encore faites                                                                                                                                                 |

Jetons : `space`, `gutter`, `radius`, `type`, `shadow`, `springs` (`default`, `snappy`, `gentle`, `press`). Le mouvement passe par Reanimated ; `useReduceMotion()` et `useReduceTransparency()` donnent les réglages d'accessibilité.

## Recettes

**Ajouter un écran.** 1) Écrivez l'écran dans `src/features/<fonction>/MonEcran.tsx` (commencez par `<Screen>`), exportez-le depuis l'`index.ts` du dossier. 2) Créez la route : `app/<chemin>.tsx` avec `export { MonEcran as default } from '@/features/<fonction>'`. 3) S'il faut un titre ou une présentation (modale, feuille), déclarez-le dans le `_layout.tsx` du dossier (`<Stack.Screen name="..." options={{ title: t('...') }} />`). 4) Naviguez avec `router.push('/chemin')`. Tous les textes passent par `t()`.

**Ajouter un réglage.** 1) `packages/shared/src/mobile/settings.ts` : le champ dans `MobileSettings`, sa valeur par défaut, son assainissement dans `sanitizeSettings`. 2) Un test dans `settings.test.ts`. 3) Une ligne dans `src/features/settings/SettingsScreen.tsx` (`SwitchRow`, `ListRow`…) qui appelle `setSettings({ champ })`. 4) Les textes dans les deux `mobile.json`. Aucun changement de store ni de sauvegarde : les réglages sont assainis à la lecture, une ancienne sauvegarde reçoit la valeur par défaut.

**Ajouter une action au store.** 1) La logique pure (une fonction qui reçoit l'état et renvoie le nouvel état) dans `packages/shared/src/mobile/<module>.ts`, avec son test. 2) Le type de l'action dans `src/state/types.ts`, dans la tranche concernée. 3) Son implémentation (une ligne `set((s) => ({ ... }))`) dans `src/state/slices/<tranche>.ts`. Une nouvelle donnée à sauvegarder : ajoutez-la à `LoadedState` et `PersistedState` dans `persist.ts`, assainissez-la dans `parsePersisted`, et si le format casse l'ancien, augmentez `PERSIST_VERSION` et ajoutez une étape dans `migrate`.

**Ajouter une icône, un composant.** Icône : `src/ui/icons.ts`. Composant partagé : `src/ui/MonComposant.tsx` et une ligne dans `src/ui/index.ts`. Un composant propre à une fonction reste dans `src/features/<fonction>/`.

## Développement : atteindre un état par lien

Le simulateur ne sait pas toucher l'écran en ligne de commande. En développement (`__DEV__`), la route `app/dev/[action].tsx` prépare l'état de l'app et renvoie à l'accueil ; elle ne fait rien en production. On l'ouvre avec un lien profond :

```bash
xcrun simctl openurl booted "exp://127.0.0.1:8081/--/dev/seed"
xcrun simctl io booted screenshot capture.png
```

| Action                                                  | Effet                                                                                                                   |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `dev/seed`                                              | 3 espaces, onglets (favoris, épinglés, du jour), historique sur plusieurs jours, signets avec dossier ; accueil terminé |
| `dev/reset`                                             | état neuf : l'accueil (onboarding) revient                                                                              |
| `dev/lang-fr`, `dev/lang-en`                            | langue                                                                                                                  |
| `dev/theme-dark`, `dev/theme-light`, `dev/theme-system` | thème                                                                                                                   |
| `dev/private-on`, `dev/private-off`                     | navigation privée                                                                                                       |
| `dev/open?url=https://…`                                | ouvre un onglet sur cette adresse                                                                                       |
| `dev/space?i=1`                                         | active le 2e espace                                                                                                     |
| `dev/toast`                                             | affiche un toast                                                                                                        |
| `dev/download?url=…`, `dev/download-private?url=…`      | télécharge un fichier (iOS ; l'onglet privé ne l'inscrit pas dans la liste)                                             |
| `dev/pw-seed`                                           | 3 identifiants de démonstration dans le Trousseau (l'id du premier est écrit dans le journal)                           |
| `dev/blocking-on`, `dev/blocking-off`                   | blocage de contenu                                                                                                      |
| `dev/clear-site-data`                                   | efface les données de sites (natif) ; le journal donne le nombre de sites avant et après                                |

Autres liens utiles : `/?dev=menu|menupw|find|reader|linkmenu|pwsave|pwchip` (menu de la page, avec ou sans « Remplir le mot de passe » ; menu d'un appui long sur un lien ; carte « Enregistrer le mot de passe ? » ; bouton clé), `/command?newTab=1&q=swift` (saisie préremplie, pour voir les suggestions du réseau), `/?dev=menu|find|reader` (ouvre le menu de page, la recherche ou la lecture sur l'onglet affiché), `/onboarding?devPage=2`, `/tabs`, `/command?newTab=1`, `/settings/data`, `/spaces/edit/new`. Après une modification de l'état persistant, relancez Expo Go pour repartir d'une pile de navigation propre.

**Dans le simulateur, sans Expo Go** (blocage de contenu, Trousseau, icônes : modules natifs). `npx expo prebuild --platform ios` (sauvegardez `package.json` avant, prebuild réécrit ses scripts), puis `xcodebuild -workspace ios/Signet.xcworkspace -scheme Signet -configuration Debug -destination id=<udid> -derivedDataPath ios/build-sim build` et `xcrun simctl install` / `launch dev.signet.mobile`. L'app de développement charge le JavaScript depuis Metro (`npx expo start`, port 8081). Le simulateur demande « Ouvrir dans Signet ? » à chaque lien `signet://` : pour piloter l'app sans toucher l'écran, lancez `python3 scripts/qa-server.py &`, l'app (en `__DEV__`) lit une commande par seconde sur `localhost:8099` et navigue (`curl -G --data-urlencode 'p=/dev/seed' localhost:8099/set`). Face ID : `xcrun simctl spawn <udid> notifyutil -s com.apple.BiometricKit.enrollmentChanged 1 && notifyutil -p com.apple.BiometricKit.enrollmentChanged` pour inscrire un visage, `notifyutil -p com.apple.BiometricKit_Sim.pearl.match` pour le reconnaître.

## Installer l'app compilée sur un iPhone

Une vraie app sur l'écran d'accueil, qui marche sans le Mac, avec sa page dans Réglages › Apps et le choix de l'icône. Avec un identifiant Apple gratuit, elle expire au bout de 7 jours : relancez la commande.

1. Une seule fois : un identifiant Apple dans Xcode › Settings › Accounts, et le Mode développeur sur l'iPhone (Réglages › Confidentialité et sécurité).
2. iPhone branché et déverrouillé, depuis la racine : `npm run mobile:device`. Le script génère le projet natif (`ios/`, ignoré par git), compile en Release et installe l'app (premier passage : 15 à 25 minutes).
3. Au premier lancement : Réglages › Général › VPN et gestion de l'appareil › votre identifiant › Faire confiance.

L'équipe de signature est `ios.appleTeamId` dans `app.json`. Pour un autre iPhone : `IOS_DEVICE=<udid> npm run mobile:device` (`xcrun devicectl list devices` donne l'UDID).

## Essayer l'app sur un iPhone avec Expo Go

Expo Go est une app gratuite qui lance votre code sans que vous ayez à compiler ni à signer quoi que ce soit.

1. Sur l'iPhone, installez **Expo Go** depuis l'App Store. Il doit gérer le **SDK 58** (mettez-le à jour si besoin). Si votre Expo Go est trop ancien ou trop récent, l'app refusera de s'ouvrir et le dira.
2. Sur votre ordinateur, installez les dépendances (une seule fois) : `npm install`, à la racine du dépôt.
3. Toujours à la racine, lancez : `npm run mobile`. Expo démarre et affiche un **QR code** dans le terminal.
4. Sur l'iPhone, ouvrez l'app **Appareil photo** et visez le QR code. Touchez la bannière « Ouvrir dans Expo Go ». L'app se charge.
5. Modifiez un fichier de `app/mobile` ou de `packages/shared/src` : l'app se recharge toute seule.

L'iPhone et l'ordinateur doivent être sur **le même réseau Wi-Fi**. Si ce n'est pas possible (réseau d'entreprise, Wi-Fi qui isole les appareils), utilisez un tunnel :

```bash
cd app/mobile
npx expo start --tunnel
```

## Simulateur iOS (Mac seulement)

1. Installez Xcode depuis l'App Store.
2. Ouvrez Xcode, puis **Xcode › Settings › Components** : téléchargez un **runtime iOS** (le simulateur n'en a pas par défaut).
3. À la racine du dépôt : `npm run mobile:ios`. Expo ouvre le simulateur et y lance l'app.

## Android

**Essayer.** Sur un téléphone Android, installez **Expo Go** depuis le Play Store, puis lancez `npm run mobile` et scannez le QR code depuis Expo Go (il a son propre lecteur). Ou, avec un émulateur Android Studio déjà démarré : `npm run mobile:android`.

**Compiler l'app (icônes alternatives, navigateur par défaut, verrou biométrique).** Rien de tout cela n'existe dans Expo Go.

1. Installez Android Studio (SDK Android, JDK 17 fourni) et branchez un téléphone en débogage USB, ou démarrez un émulateur.
2. Depuis `app/mobile` : `npx expo prebuild --platform android` génère `android/` (ignoré par git ; ne touchez pas à `ios/`). Prebuild réécrit les scripts de `package.json` : sauvegardez-le avant et restaurez la copie ensuite (un `git checkout` perdrait les dépendances ajoutées et pas encore commitées).
3. `npx expo run:android --variant release` compile et installe, ou ouvrez `android/` dans Android Studio. Pour un `.apk` / `.aab` distribuable : EAS (`npx eas-cli build --platform android --profile preview`, un `eas.json` reste à créer) ou `cd android && ./gradlew assembleRelease` (il faut alors une clé de signature).

**Clé de signature des versions publiées.** Android n'installe une mise à jour que si elle est signée avec la même clé que la version déjà installée. Toutes les versions publiées sont donc signées avec une seule clé, gardée hors du dépôt. La perdre, c'est ne plus jamais pouvoir mettre l'app à jour (il faudrait la désinstaller, données comprises).

1. Créer la clé, une seule fois (OpenSSL suffit, pas besoin de Java). Choisissez un mot de passe solide, demandé deux fois :

   ```bash
   openssl req -x509 -newkey rsa:4096 -sha256 -days 10000 -nodes -subj "/CN=Signet" -keyout signet-key.pem -out signet-cert.pem
   openssl pkcs12 -export -name signet -inkey signet-key.pem -in signet-cert.pem -out signet-release.keystore
   rm signet-key.pem signet-cert.pem
   ```

2. Sauvegarder `signet-release.keystore` et son mot de passe dans un gestionnaire de mots de passe, plus une copie hors de l'ordinateur. Ne jamais les commiter.
3. Les donner au workflow Build, sous forme de secrets du dépôt :

   ```bash
   base64 -i signet-release.keystore | gh secret set ANDROID_KEYSTORE_BASE64
   gh secret set ANDROID_KEYSTORE_PASSWORD   # colle le mot de passe
   ```

Le workflow signe alors l'APK avec cette clé (alias `signet`) et affiche dans son log l'empreinte SHA-256 du certificat, qui doit rester la même d'une version à l'autre. Une release (tag `v…`) échoue tout de suite si les secrets manquent. Un lancement à la main sans eux signe avec la clé de debug d'Expo, pour tester seulement.

**Ce qui est configuré dans `app.json` (`android`).**

| Sujet                 | Réglage                                                                                                                                                                                                                               |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Icône                 | Icône adaptative : fond, logo et silhouette monochrome (icônes à thème d'Android 13+) dans `assets/android/<id>-background\|foreground\|monochrome.png`, générés par `npm run icons -w @signet/mobile` depuis les SVG du bureau.      |
| Icônes alternatives   | Les 7 icônes (`Ocean`… `Or`) deviennent des `activity-alias` du manifeste. Sur Android, changer d'icône peut fermer l'app : l'écran de choix demande confirmation, et `useSyncAppIcon` relit l'icône réelle au lancement suivant.     |
| Permissions           | Caméra, micro, position précise et approximative (jamais en arrière-plan), biométrie, vibration, réseau. Retirées : stockage externe, médias, fenêtres flottantes. Le sélecteur de fichiers des sites passe par le sélecteur système. |
| Navigateur par défaut | Filtre d'intention `VIEW` + `BROWSABLE` + `DEFAULT` pour `http` et `https` (Paramètres › Apps › Apps par défaut › App de navigation). `app/+native-intent.tsx` renvoie ces liens vers `app/open.tsx`, qui ouvre un onglet.            |
| Clavier, retour       | `softwareKeyboardLayoutMode: resize` ; geste de retour prédictif désactivé (le bouton retour recule dans la page via `BackHandler`).                                                                                                  |
| Langues               | `expo-localization` déclare `en` et `fr` (sélecteur de langue par app d'Android 13+). Le nom de l'app est le même dans les deux.                                                                                                      |

**Pas testé (aucun appareil ni SDK Android sur le Mac de développement).** Seuls `expo export --platform android` (le bundle se construit) et `expo prebuild --platform android` (le manifeste, les ressources d'icônes et les alias se génèrent) ont été vérifiés. Restent à essayer sur un vrai téléphone : le rendu des icônes (zone sûre, icône à thème), le changement d'icône et le comportement des lanceurs, le choix de Signet comme navigateur par défaut et l'ouverture d'un lien externe, les invites caméra, micro et position dans les sites, le téléversement de fichiers, les téléchargements, le clavier (`resize` avec le mode bord à bord), le bouton retour, le verrou biométrique, la capture d'écran des onglets privés (pas encore bloquée, voir `FLAG_SECURE`).

## Téléchargements

Les téléchargements dans l'app (Bibliothèque › Téléchargements, dossier visible dans l'app Fichiers) existent **sur iOS seulement** ; sous Android, le téléchargement passe par le gestionnaire de téléchargements du système. Un téléchargement lancé depuis un **onglet privé** n'est ni listé ni sauvegardé : le fichier va dans le cache de l'app (purgé par le système) et le toast propose de l'ouvrir ou de l'enregistrer dans Fichiers.

## Commandes utiles

Depuis la racine du dépôt :

| Commande                 | Effet                                                                  |
| ------------------------ | ---------------------------------------------------------------------- |
| `npm run mobile`         | Démarre Expo (QR code).                                                |
| `npm run mobile:ios`     | Démarre Expo et ouvre le simulateur iOS.                               |
| `npm run mobile:android` | Démarre Expo et ouvre Android.                                         |
| `npm run typecheck`      | Vérifie les types, mobile compris.                                     |
| `npm run lint`           | ESLint (mobile compris : `app/mobile/app` et `app/mobile/src`).        |
| `npm run i18n:check`     | Vérifie les traductions et l'absence de texte en dur (mobile).         |
| `npm test`               | Tests Vitest (la logique du mobile est testée dans le paquet partagé). |

## Limites connues

- **Expo Go d'abord.** Préférez les bibliothèques incluses dans Expo Go (`npx expo install` depuis `app/mobile`). Une fonction native hors Expo Go (comme les icônes de l'app) doit se désactiver proprement dans Expo Go. Pas encore de `.apk` ni de publication.
- **iOS utilise WebKit.** Apple impose son moteur web à tous les navigateurs iOS : les fonctions du bureau qui dépendent de Chromium (extensions, bloqueur de pubs au niveau du moteur…) ne viendront pas telles quelles.
- **Une seule copie de React.** `metro.config.js` force le bundler à prendre le `react` de l'app mobile (19.3.0, celui qu'Expo attend), et non celui du bureau remonté à la racine par npm. Conséquence : `npx expo-doctor` signale un `react` en double. **C'est attendu**, pas une erreur.
- **Le flou** de `GlassBar` n'existe que sur iOS ; sous Android, un voile presque opaque le remplace.
