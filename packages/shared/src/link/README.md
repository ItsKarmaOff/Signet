# Lien entre appareils (sans compte)

Un téléphone s'appaire **une fois** avec un ordinateur en scannant un QR code. Ensuite, quand ils sont sur le même réseau :

- `localhost:PORT` tapé sur le téléphone ouvre le serveur de dev de l'ordinateur. L'origine reste `http://localhost:PORT`, donc la page est un « contexte sûr » ; les WebSocket (rechargement auto Vite/Next) et les appels vers d'autres ports locaux marchent aussi ;
- « Ouvrir sur mon téléphone » (ordinateur → téléphone) et « Ouvrir sur l'ordinateur » (téléphone → ordinateur) ;
- le téléphone voit la liste des serveurs de dev de l'ordinateur (ports qui écoutent + onglets localhost ouverts).

Pas de compte, pas de serveur, rien ne passe par Internet. Le code commun est ici (`protocol.ts`, `status.ts`, `loopback.ts`) ; chaque app fait son transport.

## Interrupteurs (des deux côtés)

- Un **interrupteur général** (« Synchro ») et un **interrupteur par appareil**.
- Un lien ne fonctionne que si les **quatre** sont sur ON. Couper ne supprime pas le lien (les clés restent).
- Interrupteur général OFF : ceux des appareils sont grisés mais gardent leur position.
- **Oublier** un appareil supprime le lien (clés comprises) ; on prévient l'autre (`forget`) s'il est connecté. Sinon, il l'apprendra
  à la prochaine connexion (`unknown-link`) et affichera « Lien plus valide » avec un bouton pour le supprimer de son côté.
- Réappairer = nouveau lien = nouvelles clés (certificat et jeton neufs).
- État affiché : `linkStatus()` dans `status.ts` (Connecté / Connexion… / En pause ici / En pause sur X / Introuvable / Lien plus valide).

## Sécurité

- **Un certificat TLS par lien** (auto-signé, ECDSA P-256, créé par l'ordinateur à l'appairage). Le téléphone l'épingle : SHA-256 du
  certificat DER, reçu dans le QR code. Aucune autre vérification (pas d'autorité, pas de nom d'hôte) : seule l'empreinte compte.
- **SNI** = `serverNameForLink(linkId)`. L'ordinateur choisit le certificat du lien d'après ce nom. Nom inconnu → certificat par défaut
  de l'installation ; le téléphone voit alors une empreinte différente (`pin-mismatch`). Si ce même ordinateur (même `desktopId` en
  mDNS) répond ainsi, le lien est considéré comme supprimé de l'autre côté (« Lien plus valide »).
- **Jeton du lien** : 32 octets aléatoires (base64url, 43 caractères), créés par l'ordinateur et donnés au téléphone dans `paired`.
  Le téléphone le garde dans le Trousseau / Keystore (expo-secure-store). L'ordinateur ne garde que son SHA-256 et compare en temps constant.
- **Code d'appairage** : 16 octets aléatoires, usage unique, valable 5 minutes (`PAIRING_TTL_MS`). Un seul QR code actif à la fois.
- L'ordinateur n'ouvre un tunnel que vers **127.0.0.1:port ou [::1]:port**, jamais ailleurs, et jamais vers son propre port de lien.
- Interrupteur général OFF sur l'ordinateur : il arrête l'annonce mDNS **et** ferme son port d'écoute. Les connexions ouvertes
  reçoivent `{"type":"status","paused":true}` puis sont fermées.

## Découverte

- L'ordinateur s'annonce en mDNS : type `_signet._tcp` (`LINK_SERVICE_TYPE`), nom d'instance = nom de l'ordinateur, TXT
  `v=1` et `id=<desktopId>`.
- Il écoute sur toutes les interfaces, port `LINK_DEFAULT_PORT` (47615) s'il est libre, sinon un port libre (annoncé partout).
- Le téléphone essaie dans l'ordre : adresses vues en mDNS pour ce `desktopId`, puis les dernières adresses qui ont marché, puis
  celles du QR code. Reconnexion avec attente croissante (1 s → 30 s), seulement app au premier plan.

## Connexions et messages

Une connexion TCP + TLS = un rôle. Chaque message est un objet JSON sur une ligne (`\n`), au plus 64 Kio. Types et validation :
`protocol.ts` (`parseLinkMessage`, `encodeLinkMessage`). Le téléphone parle en premier.

### Appairage

1. L'ordinateur crée un lien en attente : `linkId` (8 octets aléatoires en hexadécimal), certificat, code. Il affiche le QR code :
   `encodePairingUrl(offer)` → `signet://link?d=…` (l'appareil photo d'iOS ouvre aussi l'app avec ce lien).
2. Le téléphone lit le QR code (`decodePairingUrl`), ouvre une socket épinglée sur `hosts[i]:port` et envoie
   `{"type":"pair","v":1,"linkId","code","device"}`.
3. L'ordinateur vérifie le code (temps constant, usage unique, pas expiré), crée le jeton, enregistre le lien (ON par défaut) et répond
   `{"type":"paired","token","device"}` puis ferme. Erreurs : `bad-code`, `expired`, `unknown-link`, `version`.
4. Le téléphone enregistre le lien (ON par défaut) puis ouvre la connexion de contrôle.

### Contrôle

- Téléphone : `{"type":"hello","v":1,"linkId","token","role":"control","device"}`.
- Ordinateur : `{"type":"welcome","device","paused"}` (`paused` = ce lien est en pause côté ordinateur), ou `error`
  (`unknown-link`, `bad-token`, `version`) puis fermeture.
- Ensuite, dans les deux sens : `status`, `open`, `forget`, `ping`/`pong`. L'ordinateur envoie `servers` juste après `welcome` puis à
  chaque changement (au plus une fois toutes les 2 s), seulement si le lien est actif des deux côtés.
- `ping` toutes les 20 s par le téléphone ; sans réponse en 10 s, il ferme et se reconnecte.
- Pause côté ordinateur (interrupteur de l'appareil) : il envoie `status paused:true` et garde la connexion ouverte (il ne répond plus
  qu'aux `ping` et ne pousse plus rien). Reprise : `status paused:false` puis `servers`.
- Pause côté téléphone (général ou appareil) : il envoie `status paused:true` puis ferme, et ne se reconnecte plus à cet ordinateur.
  Reprise : il se reconnecte (le `hello` vaut reprise).
- `open` n'est accepté que si le lien est actif des deux côtés. URL http(s) seulement.

### Tunnel (localhost)

- Téléphone : `{"type":"hello","v":1,"linkId","token","role":"tunnel","port":5173}`.
- Ordinateur : vérifie le jeton et que le lien est ON chez lui, se connecte à `127.0.0.1:port` puis `[::1]:port` (Vite écoute souvent
  sur `::1` seulement), répond `{"type":"ok"}` puis relaie les octets bruts dans les deux sens jusqu'à la fermeture.
  Erreurs : `paused`, `port-closed`, `port-refused`, `bad-token`, `unknown-link`.
- Une connexion TLS par tunnel (le WebView garde ses connexions ouvertes, ça suffit sur un réseau local).

## Comment le téléphone envoie `localhost` à l'ordinateur

`servers` porte aussi `ports` : tous les ports qui écoutent sur l'ordinateur (pages et API, hors ports système et éphémères). Le
téléphone écoute lui-même sur son localhost à ces ports (« ports miroirs ») et relaie chaque connexion dans un tunnel : un vrai iPhone
n'envoie jamais `localhost` à un proxy, et Android fait de même pour rester simple. Détails : `app/mobile/modules/signet-link/README.md`.

## Choix de l'ordinateur pour `localhost`

`pickLocalhostRoute(actifs, choix)` : aucun → `localhost` reste le téléphone ; un seul → lui ; plusieurs sans choix valable → le
téléphone affiche une fenêtre pour choisir (quand on ouvre une adresse `localhost`), et garde ce choix tant que cet ordinateur est actif.
Sur le téléphone, les adresses `127.0.0.1`, `[::1]` et `0.0.0.0` sont ramenées sur `localhost` (`toLinkLocalhost`) : seul ce nom
passe par l'ordinateur.
