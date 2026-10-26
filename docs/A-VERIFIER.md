# À vérifier à la main

Ce qui n'a pas pu être testé automatiquement : il faut une vraie souris, un vrai trackpad, un vrai clavier ou un vrai compte. Cochez au fur et à mesure.

## Raccourcis modifiables (Réglages › Raccourcis)

- [ ] Changer un raccourci (par exemple « Vue partagée » sur ⌘E), puis l'utiliser : la commande se déclenche.
- [ ] Pendant l'enregistrement d'un raccourci, ⌘W ne ferme pas la fenêtre et ⌘Q ne quitte pas.
- [ ] Après « Tout rétablir », les raccourcis d'origine marchent à nouveau.

## Dossiers dynamiques

- [ ] Réglages › Général › Dossiers dynamiques : enregistrer un jeton GitHub (portée `repo`).
- [ ] Créer un dossier « GitHub › Revues demandées » et un « Mes PR ouvertes » : ils se remplissent.
- [ ] Laisser Signet ouvert plus de 15 minutes : un dossier de flux se met à jour seul.

## Notes et Easels

- [ ] Dans un Easel, défiler à deux doigts et zoomer en pinçant au trackpad.
- [ ] Glisser une image depuis le Finder dans un Easel, et coller une image copiée.
- [ ] Dans une note, ⌘K sur un mot sélectionné crée un lien.

## Sauvegarde (Réglages › Sauvegarde)

- [ ] Exporter avec les mots de passe : Touch ID est demandé, le fichier `.signetbackup` est créé.
- [ ] Importer ce fichier en mode « Ajouter » : la confirmation s'affiche, Signet redémarre, rien n'est en double.
- [ ] Importer en mode « Remplacer », puis restaurer la copie de sécurité depuis la même section.
- [ ] Importer une sauvegarde contenant des extensions : elles se réinstallent au redémarrage.

## Mots de passe : bouton clé

- [ ] Se connecter à un site, cliquer ailleurs quand la proposition d'enregistrer apparaît : la clé reste dans la barre d'adresse, un clic rouvre la proposition.
- [ ] Sur un site déjà enregistré, la clé liste les identifiants ; « Copier » met l'identifiant dans le presse-papiers.

## Apparence

- [ ] Un espace en dégradé avec du grain : le grain est visible à l'œil.
- [ ] L'aperçu au survol n'apparaît pas pour l'onglet en cours, ni pour les panneaux d'une vue partagée.

## Interface qui disparaît

- [ ] Si la barre latérale et la barre d'outils disparaissent : l'interface doit revenir seule. Sinon, Affichage › Recharger l'interface.
- [ ] Après un tel incident, envoyer le fichier `~/Library/Application Support/Signet/logs/interface.log` : il dit ce qui a planté.

## Quitter, mini-lecteur, extensions

- [ ] Appuyer une fois sur ⌘Q : la bulle « Maintenir ⌘Q pour quitter » apparaît ; maintenir ⌘Q quitte l'app.
- [ ] Lancer une vidéo YouTube puis changer d'onglet : le mini-lecteur apparaît ; il se déplace et se redimensionne à la souris.
- [ ] Glisser un bouton d'extension épinglé dans la barre d'outils pour changer sa place ; un clic ouvre bien la fenêtre de l'extension.

## Passkeys depuis un téléphone (Linux ; macOS en développement avec `SIGNET_WEBAUTHN_INTERCEPT=1`)

- [ ] Sur webauthn.io, « Register » : la fenêtre « Clé d'accès » s'ouvre avec un QR code ; « Annuler », Échap et la fermeture de la fenêtre rendent la main au site (erreur).
- [ ] Bluetooth du PC désactivé : la fenêtre le signale ; une fois réactivé, le message normal revient.
- [ ] Scanner le QR avec un téléphone (iPhone ou Android), accepter : « Téléphone trouvé. Connexion… », puis « Confirmez sur votre téléphone », puis la fenêtre se ferme et webauthn.io confirme l'enregistrement.
- [ ] « Authenticate » sur webauthn.io avec la passkey créée : la connexion réussit.
- [ ] Connexion Microsoft avec une passkey du téléphone ; sans téléphone, « Annuler » mène à « Use my password ».
- [ ] La même chose dans une fenêtre privée : la fenêtre du QR s'ouvre au-dessus de la fenêtre privée.
- [ ] Linux : à vérifier en premier (le Mac a déjà été validé), la détection du téléphone par BlueZ ; en cas d'échec, noter les lignes `Passkey:` et `Bluetooth:` du terminal (`npm run dev`).
