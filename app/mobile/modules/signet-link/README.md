# signet-link (module natif)

Transport du lien téléphone ↔ ordinateur. Protocole et règles : `packages/shared/src/link/README.md`. Interface JS :
`src/SignetLinkModule.ts` (c'est le contrat : iOS et Android l'implémentent à l'identique).

## Sockets TLS épinglées (`open`, `writeLine`, `close`, `onSocketLine`, `onSocketClosed`)

- TLS 1.2+ vers `host:port`, SNI = `serverName`. La vérification standard est **remplacée** par : SHA-256 du certificat feuille (DER)
  en base64url == `certSha256`. Sinon : rejet `pin-mismatch`.
  - iOS : Network.framework, `NWProtocolTLS.Options` + `sec_protocol_options_set_verify_block` + `sec_protocol_options_set_tls_server_name`.
  - Android : `SSLSocket` (SSLContext avec un X509TrustManager qui compare l'empreinte), SNI via `SSLParameters.serverNames`.
- `open` résout après la poignée de main ; rejette (`timeout`, `refused`, `unreachable`, `pin-mismatch`, `tls`) sinon.
- Lecture par lignes (`\n`), UTF-8 ; une ligne de plus de 64 Kio ferme la socket (`line-too-long`).
- `onSocketClosed` est émis une seule fois par socket, avec `error` sauf fermeture normale ou `close()`.
- Tout le réseau hors du fil principal ; événements émis vers JS sans bloquer.

## mDNS (`startBrowsing`, `stopBrowsing`, `onServiceFound`, `onServiceLost`)

- Type `_signet._tcp`, domaine `local.`. Pour chaque service : résoudre adresses + port + TXT (`v`, `id`).
- iOS : `NWBrowser` (bonjourWithTXTRecord) puis résolution (NWConnection vers l'endpoint du service, ou `NetService`), Info.plist
  `NSBonjourServices` et `NSLocalNetworkUsageDescription` (déjà dans app.json). La première recherche déclenche l'alerte « Réseau local ».
- Android : `NsdManager` (discoverServices + resolveService / registerServiceInfoCallback sur API 34+), `WifiManager.MulticastLock`
  pendant la recherche.

## Relais localhost (`setLocalhostRoute`, `attachToWebView`, `isLocalhostRoutingSupported`, `onTunnelError`)

Ouvrir le tunnel (commun) : socket épinglée vers le premier endpoint de la route qui répond (`endpoints`, dans l'ordre ; retenir celui
qui a marché), ligne `{"type":"hello","v":1,"linkId","token","role":"tunnel","port":PORT}`, attendre une ligne. `{"type":"ok"}` →
relayer les octets. Sinon, répondre `HTTP/1.1 502 Bad Gateway` avec une petite page texte, et émettre `onTunnelError { port, code }`.

### iOS : ports miroirs

**Sur un vrai iPhone, iOS n'envoie jamais `localhost` à un proxy** (`WKWebsiteDataStore.proxyConfigurations` est ignoré pour lui ;
vérifié sur un iPhone sous iOS 27 : `example.com` arrive au proxy, `localhost` jamais). Le simulateur, qui utilise le réseau du Mac,
le fait : il ne prouve donc rien pour ce cas.

Le module écoute donc lui-même sur le localhost du téléphone (`127.0.0.1` et `::1`), aux ports `route.ports` (tous les ports qui
écoutent sur l'ordinateur, plus ceux ouverts à la main), et passe chaque connexion telle quelle dans un tunnel vers le même port de
l'ordinateur. Le WebView ouvre `http://localhost:5173` sans rien savoir : origine `localhost`, contexte sûr, WebSocket, appels vers
d'autres ports (API) compris, onglets privés aussi (`attachToWebView` ne fait rien). Un port déjà pris sur le téléphone est ignoré.
Les écoutes sont relancées au retour au premier plan (iOS peut les reprendre pendant la mise en veille). Dans le simulateur, rien
n'est relayé : il partage le localhost du Mac (le relais s'y rappellerait lui-même) et `localhost` y mène déjà à l'ordinateur.
Journal : sous-système `dev.signet.link`, ou sortie standard avec `SIGNET_LINK_DEBUG=1`
(`DEVICECTL_CHILD_SIGNET_LINK_DEBUG=1 xcrun devicectl device process launch --console --device <udid> dev.signet.mobile`).

### Android : ports miroirs aussi (jamais testé sur un appareil)

Même principe qu'iOS : un `ServerSocket` sur `127.0.0.1` et `::1` pour chaque port de `route.ports`, chaque connexion relayée telle
quelle dans un tunnel. L'émulateur Android a son propre localhost : le relais y marche comme sur un téléphone. Le trafic `http://`
(localhost compris) est autorisé par `expo-build-properties` (`android.usesCleartextTraffic`).
