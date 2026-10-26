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
