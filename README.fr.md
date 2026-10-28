<p align="center">
  <img src="app/desktop/resources/icons/aurore.png" alt="Icône de Signet" width="128" height="128">
</p>

<h1 align="center">Signet</h1>

<p align="center">
  Un navigateur centré sur une barre latérale, pour macOS, Linux et Windows, avec une app compagnon pour iOS et Android.
</p>

<p align="center">
  <a href="https://github.com/ItsKarmaOff/Signet/actions/workflows/ci.yml"><img src="https://github.com/ItsKarmaOff/Signet/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="Licence : GPL-3.0-or-later"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <b>Français</b>
</p>

<p align="center">
  <img src="screenshots/signet.webp" alt="Signet sur macOS : barre latérale avec favoris, dossier Reading list, onglets épinglés et du jour, barre de signets, et un article Wikipédia ouvert" width="900">
</p>

---

Signet range les onglets dans une barre latérale verticale, les regroupe en espaces, et laisse le plus de place possible à la page. L'app de bureau est écrite avec Electron, React et TypeScript. L'app mobile est en React Native avec Expo, et partage son modèle de données avec le bureau.

Le projet est jeune et avance vite. Il reste des aspérités : [signalez-les](https://github.com/ItsKarmaOff/Signet/issues).

## Fonctions

Les raccourcis ci-dessous sont ceux de macOS. Sous Linux et Windows, <kbd>⌘</kbd> devient <kbd>Ctrl</kbd>. Tous se modifient dans les Réglages.

**Onglets et espaces**

- **Barre latérale et espaces** : onglets verticaux, favoris, onglets épinglés, onglets du jour archivés automatiquement. Un espace par contexte, avec sa couleur ou son dégradé (2 ou 3 couleurs et un grain).
- **Barre de commande** (<kbd>⌘T</kbd>, <kbd>⌘L</kbd>) : onglets ouverts, historique, archives, actions et recherche au même endroit.
- **Vue partagée** (<kbd>⌃⇧=</kbd>) : deux ou trois pages côte à côte.
- **Peek** (<kbd>⇧</kbd> clic) : aperçu d'un lien par-dessus la page.
- **Little Signet** (<kbd>⌥⌘N</kbd>) : petite fenêtre pour un lien rapide, à rapatrier dans la fenêtre principale.
- **Aperçu des onglets** au survol, mise en veille des onglets inutilisés, annulation d'une fermeture ou d'un archivage.
- **Dossiers dynamiques** : un dossier qui se remplit seul depuis un flux RSS ou Atom, ou depuis vos demandes de fusion GitHub.
- **Notes et Easels** (<kbd>⌃⌘N</kbd>) : des notes et des tableaux libres (textes, dessins, images) rangés dans la barre latérale comme des onglets.

**Navigation**

- **Bloqueur de pubs** intégré et **extensions du Chrome Web Store**.
- **Mots de passe** : trousseau de Signet chiffré par le trousseau du système, ou le gestionnaire de son choix (Bitwarden, 1Password…).
- **Mode lecture** (<kbd>⌥⌘R</kbd>) et **traduction** des pages, sur place.
- **Imprimer** (<kbd>⌘P</kbd>), exporter en PDF, enregistrer la page, afficher le code source (<kbd>⌥⌘U</kbd>), corrections d'orthographe au clic droit.
- **Capture d'écran** (<kbd>⇧⌘2</kbd>) : une zone, la partie visible ou la page entière, à copier ou enregistrer.
- **Apps** : installer un site comme une app, à la façon de Chrome : sa propre fenêtre, son icône dans le Dock, Launchpad et Spotlight.

**Personnalisation**

- **Boosts** : retouches par site (CSS, mode sombre, éléments masqués).
- **Mods** : du CSS pour personnaliser l'interface de Signet. Si un mod la rend inutilisable, démarrez avec `--safe-mode`.
- **Icône de l'app** : huit icônes au choix.
- **Sauvegarde** : exporter et importer sa configuration (réglages, espaces, signets, mots de passe chiffrés…), partie par partie.
- **Interface en anglais et en français.**

**Pour les développeurs web**

- Outils de développement intégrés, mode développeur pour les pages locales (compteur d'erreurs de console, pages locales hors de l'historique), mode responsive, visionneuse JSON.
- **Lien avec le téléphone** : appairage par QR code, sans compte, pour ouvrir le `localhost` de l'ordinateur sur le téléphone.

**App mobile (iOS et Android)**

- Navigateur, barre de commande, onglets, espaces, bibliothèque, réglages, onglets privés verrouillés par biométrie, bloqueur de pubs, mots de passe, téléchargements (iOS) et icônes des sites.
- Sur iOS, Apple impose son moteur WebKit à tous les navigateurs : les fonctions du bureau qui dépendent de Chromium (les extensions, par exemple) ne viendront pas telles quelles.

## Installer

Téléchargez le fichier de votre système dans la [dernière version](https://github.com/ItsKarmaOff/Signet/releases/latest) :

| Système                                | Intel / AMD (64 bits)          | ARM (64 bits)                                      |
| -------------------------------------- | ------------------------------ | -------------------------------------------------- |
| macOS                                  | `Signet-X.Y.Z-x64.dmg`         | `Signet-X.Y.Z-arm64.dmg` (Apple Silicon : M1, M2…) |
| Windows                                | `Signet-X.Y.Z-x64-setup.exe`   | -                                                  |
| Linux, toutes distributions (AppImage) | `Signet-X.Y.Z-x86_64.AppImage` | `Signet-X.Y.Z-arm64.AppImage`                      |
| Linux : Debian, Ubuntu, Mint (`deb`)   | `Signet-X.Y.Z-amd64.deb`       | `Signet-X.Y.Z-arm64.deb`                           |
| Linux : Fedora, openSUSE (`rpm`)       | `Signet-X.Y.Z-x86_64.rpm`      | `Signet-X.Y.Z-aarch64.rpm`                         |
| Android                                | -                              | `Signet-X.Y.Z-android-arm64.apk`                   |

Les apps ne sont pas encore signées avec un certificat payant, votre système vous avertira donc la première fois :

- **macOS** : ouvrez l'app une fois, fermez l'avertissement, puis allez dans **Réglages Système › Confidentialité et sécurité** et cliquez sur **Ouvrir quand même**.
- **Windows** : sur l'avertissement SmartScreen, cliquez sur **Informations complémentaires › Exécuter quand même**.
- **Android** : autorisez l'installation depuis des sources inconnues.

Sur Linux :

- **AppImage** : rendez-la exécutable (`chmod +x Signet-*.AppImage`), puis lancez-la. Elle s'ajoute au menu des applications et se met à jour toute seule. Si elle ne démarre pas, installez FUSE (sur Ubuntu 24.04 et suivantes : `sudo apt install libfuse2t64`).
- **deb** : `sudo apt install ./Signet-X.Y.Z-amd64.deb`. **rpm** : `sudo dnf install ./Signet-X.Y.Z-x86_64.rpm`. Signet apparaît alors dans le menu des applications. Quand une nouvelle version sort, Signet vous prévient : installez-la de la même façon.

## Compatibilité

**Ordinateur**

| Système    | Processeur          | Paquet                 | Testé         |
| ---------- | ------------------- | ---------------------- | ------------- |
| macOS      | Apple Silicon       | `.dmg`                 | ✅ macOS 27   |
| macOS      | Intel               | `.dmg`                 | ⚠️ Pas encore |
| Windows 11 | Intel / AMD 64 bits | Installateur `.exe`    | ⚠️ Pas encore |
| Linux      | Intel / AMD 64 bits | AppImage, `deb`, `rpm` | ⚠️ Pas encore |
| Linux      | ARM 64 bits         | AppImage, `deb`, `rpm` | ⚠️ Pas encore |

**Mobile**

| Système | Paquet                                                                                | Testé         |
| ------- | ------------------------------------------------------------------------------------- | ------------- |
| iOS     | Pas encore sur l'App Store : à compiler soi-même ([app/mobile](app/mobile/README.md)) | ✅ iOS 27     |
| Android | `.apk` (ARM 64 bits)                                                                  | ⚠️ Pas encore |

« Pas encore » veut dire que l'app est construite pour ce système et passe les tests automatiques (tests unitaires sur Windows et Linux, tests de bout en bout sur Linux), mais que personne ne l'y a encore utilisée. Les autres versions de macOS et d'iOS n'ont pas été essayées non plus. Si vous lancez Signet sur l'une d'elles, [dites-nous comment ça se passe](https://github.com/ItsKarmaOff/Signet/issues).

## Lancer depuis les sources

Il faut [Node.js](https://nodejs.org/) 24 (la version de la CI) et npm.

```bash
git clone https://github.com/ItsKarmaOff/Signet.git
cd Signet
npm install
npm run dev        # app de bureau avec rechargement à chaud
```

Si l'app ne démarre pas parce que le binaire d'Electron manque (le fichier `node_modules/electron/path.txt` n'existe pas), installez-le :

```bash
node node_modules/electron/install.js
```

### Mobile

```bash
npm run mobile     # démarre Expo et affiche un QR code
```

Installez **Expo Go** (SDK 58) sur votre téléphone, puis scannez le QR code. Le téléphone et l'ordinateur doivent être sur le même Wi-Fi. La compilation de l'app, le simulateur iOS, Android et les limites connues sont dans [app/mobile/README.md](app/mobile/README.md).

## Structure du dépôt

Le dépôt est un monorepo npm (workspaces) : un seul `npm install` à la racine pour les trois parties.

```
app/desktop/       l'app de bureau (Electron + React + TypeScript), pour macOS, Linux et Windows
app/mobile/        l'app mobile (React Native + Expo), pour iOS et Android
packages/shared/   TypeScript neutre vis-à-vis de la plateforme, commun au bureau et au mobile
docs/              architecture, développement, spécification
```

`app/desktop` et `app/mobile` importent `packages/shared`, jamais l'inverse. `packages/shared` ne peut importer ni Electron, ni Node, ni le DOM, ni React Native : son `tsconfig` n'a pas ces types, un import interdit fait échouer la vérification des types.

## Commandes

Toutes se lancent depuis la racine.

| Commande                 | Effet                                                                |
| ------------------------ | -------------------------------------------------------------------- |
| `npm run dev`            | Lance l'app de bureau avec rechargement à chaud.                     |
| `npm run build`          | Compile l'app de bureau (dans `app/desktop/out`).                    |
| `npm start`              | Lance l'app de bureau compilée.                                      |
| `npm run typecheck`      | Vérifie les types (bureau, paquet partagé, mobile).                  |
| `npm test`               | Tests unitaires (bureau et paquet partagé).                          |
| `npm run test:e2e`       | Tests de bout en bout (Playwright).                                  |
| `npm run lint`           | ESLint sur tout le dépôt.                                            |
| `npm run format`         | Met en forme avec Prettier (`format:check` pour seulement vérifier). |
| `npm run i18n:check`     | Vérifie les traductions (mêmes clés en anglais et en français).      |
| `npm run icons`          | Régénère les icônes de l'app.                                        |
| `npm run dist`           | Construit le `.dmg` macOS (dans `app/desktop/dist`).                 |
| `npm run dist:linux`     | Construit l'AppImage Linux.                                          |
| `npm run dist:win`       | Construit l'installateur Windows.                                    |
| `npm run mobile`         | Démarre Expo pour l'app mobile.                                      |
| `npm run mobile:ios`     | Ouvre l'app mobile dans le simulateur iOS.                           |
| `npm run mobile:android` | Ouvre l'app mobile sur Android.                                      |
| `npm run mobile:device`  | Compile et installe l'app mobile sur un iPhone branché.              |

## Documentation

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) : la carte du code, pour un nouveau contributeur.
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) : scripts de développement, tests, variables `SIGNET_*`.
- [docs/SPEC.md](docs/SPEC.md) : le comportement attendu, version par version.
- [docs/MANUAL-CHECKS.md](docs/MANUAL-CHECKS.md) : ce qui se vérifie à la main.
- [app/mobile/README.md](app/mobile/README.md) et [packages/shared/README.md](packages/shared/README.md).

## Contribuer

Les contributions sont les bienvenues : signalements de bugs, corrections, fonctions, traductions, documentation. Lisez [CONTRIBUTING.md](CONTRIBUTING.md) avant d'ouvrir une pull request. Pour une faille de sécurité, suivez [SECURITY.md](SECURITY.md) plutôt que d'ouvrir une issue publique. Chacun s'engage à respecter le [code de conduite](CODE_OF_CONDUCT.md).

## Licence

Signet est un logiciel libre distribué sous licence [GNU GPL v3 ou ultérieure](LICENSE).
