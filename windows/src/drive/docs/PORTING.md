# Files → Rust : plan de portage

Portage 1:1 de [Files](https://github.com/files-community/Files) (C#/.NET WinUI 3,
~130k lignes) vers Rust avec UI Win32 pure (`windows-rs` + Direct2D/DirectComposition).
Source de référence : `C:\Users\martinien\projects\Files-main`.

## Correspondance projets C# → crates Rust

| Projet C# | Crate Rust | Statut |
|---|---|---|
| Files.Shared | `drive-shared` | porté (12 tests) |
| Files.Core.Storage (+ modèle OwlCore.Storage) | `drive-core-storage` | porté (traits) |
| Files.App.Storage | `drive-app-storage` | porté (23 tests) : IShellItem, IFileOperation+sink, helpers Shell, icônes/thumbnails, watcher SHChangeNotify, STA, HomeFolder, FTP (feature) |
| Files.App.Controls | `drive-app-controls` (contrôles Direct2D custom) | à faire |
| Files.App | `drive-app` | fonctionnel : fenêtre custom frame + Mica + bande accent, onglets style navigateur, navigation complète (historique, breadcrumb cliquable), vue détails, vraies icônes Shell (IconCache), menu contextuel Shell natif, assets originaux (Home/Star/Folder png), opérations fichiers (Ctrl+C/X/V presse-papiers CF_HDROP compatible Explorateur, Suppr → corbeille, Ctrl+Maj+N, F5, Alt+←/→, Ctrl+T/W) via IFileOperation, argument ligne de commande = dossier à ouvrir, DPI per-monitor V2, page Paramètres (thème système/clair/sombre appliqué en direct, éléments masqués, extensions — persistés dans %LOCALAPPDATA%\FilesRust\settings.json) |
| Files.App.CsWin32 | remplacé par le crate `windows` (features par module) | n/a |
| Files.Core.SourceGenerator | macros proc + `build.rs` (voir Codegen) | localisation faite (drive-localization) |
| Files.App.Server | binaire serveur COM out-of-proc (plus tard) | à faire |
| Files.App.Launcher | binaire shim protocole (plus tard) | à faire |
| Files.App.OpenDialog / SaveDialog | DLL COM in-proc (plus tard, très risqué : IFileDialogPrivate) | à faire |
| Files.App.BackgroundTasks | tâche WinRT (plus tard) | à faire |

## Décisions d'architecture

- **UI** : Win32 pur via `windows-rs`. Fenêtre + Mica (`DwmSetWindowAttribute`
  DWMWA_SYSTEMBACKDROP_TYPE), rendu Direct2D/DirectWrite sur swapchain
  DirectComposition (alpha prémultiplié pour laisser transparaître Mica),
  DPI per-monitor v2. Contrôles owner-drawn : TabBar, Omnibar/Breadcrumb,
  Sidebar, DetailsView/GridView virtualisés, page Accueil (cartes accès rapide,
  jauges lecteurs — cible visuelle : capture d'écran fournie par l'utilisateur).
- **MVVM → Rust** : ViewModels = structs d'état ; `WeakReferenceMessenger` →
  bus d'événements (channels) ; DI `Ioc.Default` → struct `Services` partagé (Arc).
- **Actions/Commands** : ~172 actions C# avec `[GeneratedRichCommand]` →
  enum `CommandCode` + table statique code → `Box<dyn Action>` (macro).
- **Virtualisation listes** : modèle owner-data ; énumération streamée
  (`FindFirstFileEx`/`IEnumShellItems`) sur thread worker + chargement paresseux
  des icônes via `IShellItemImageFactory`.
- **COM** : interfaces windows-rs (RAII) remplacent `ComPtr<T>` ;
  `IFileOperationProgressSink` implémenté via `#[implement]` (remplace la vtable
  manuelle 19 slots du C#). Shell interactif = thread STA dédié
  (`CoInitializeEx(COINIT_APARTMENTTHREADED)`), équivalent de `STATask`.
- **Settings** : store JSON central (serde) + contexte partagé + notifications
  par canal (remplace `INotifyPropertyChanged` + fichier JSON unique du C#).
- **Localisation** : `build.rs` parse les `.resw` en-US (+48 langues) → constantes
  + lookup runtime avec fallback en-US (remplace StringsPropertyGenerator).

## Codegen à répliquer (Files.Core.SourceGenerator)

1. `StringsPropertyGenerator` → `build.rs` (.resw/.json → constantes).
2. `CommandManagerGenerator` → macro (enum CommandCodes + fabriques).
3. `RegistrySerializationGenerator` → derive macro registre (crate `winreg`).
4. `VTableFunctionGenerator` → vtables `#[repr(C)]` manuelles (interfaces non
   documentées : IStorageProviderStatusUI*, IOpenControlPanel, IDetectionAndSharing —
   respecter les slots 3/6/8/14).
5. DependencyPropertyGenerator (tiers, Files.App.Controls) → système de
   propriétés des contrôles custom.

## Contrats inter-processus à préserver

- Protocole `files-dev:?cmd=<args hex UTF-8>` (Launcher, Open/SaveDialog → app).
- Fichier temporaire de sortie + event nommé `FILEDIALOG` (dialogues).
- CLSID : FilesOpenDialog {DC1C5A9C-E88A-4DDE-A5A1-60F82A20AEF7},
  FilesSaveDialog {C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}.

## Ordre de portage

1. `drive-shared` (feuille, aucune dépendance) ✦ en cours
2. `drive-core-storage` : traits Storable/File/Folder/ModifiableFolder (modèle
   OwlCore) + IStorageService, watchers, StorableKind, extensions
3. `drive-app-storage` : WindowsStorable/WindowsFile/WindowsFolder (IShellItem),
   WindowsBulkOperations (IFileOperation + sink), watchers Shell
   (SHChangeNotifyRegister), helpers (icônes GDI+/thumbnails, context menus,
   quick access), FTP (crate suppaftp), corbeille
4. Socle UI Win32 : fenêtre + Mica + D2D/DComp + boucle de messages
5. Contrôles : TabBar, Omnibar, Sidebar, DetailsView/GridView virtualisés,
   page Accueil, StatusCenter
6. Services (settings, localisation, drives, réseau, tags, git via `git2`,
   cloud detectors), opérations fichiers + dialogues de collision
7. Processus satellites (Server, Launcher, dialogues COM)

## Détails clés du C# source (rappels)

- `ShellViewModel.cs` (~3080 lignes) est la pièce centrale : collection bulk
  observable + énumération incrémentale + watcher + tri/groupement/filtre.
- Énumérateurs : `Win32StorageEnumerator` (FindFirstFile, chemin rapide) et
  `UniversalStorageEnumerator` (WinRT, cloud/MTP).
- Les 4 rapports d'exploration détaillés sont dans l'historique de session du
  2026-07-13 ; re-explorer `Files-main` au besoin.

## Prochaine étape prioritaire (référence utilisateur du 2026-07-13)
La page Paramètres doit reproduire la structure originale : navigation interne
(Général, Apparence, Disposition, Fichiers et dossiers, Actions, Étiquettes,
Outils de développement, Avancé, À propos) ; page Apparence avec lignes
Thème (dropdown), Arrière-plan (Mica/Mica Alt/Acrylique), Couleurs du fond
(nuancier de dossiers colorés), Image de fond, Police. Breadcrumb « Accueil >
Paramètres ». Onglets : drag & drop implémenté (réordonnancement en direct).

## Session 2 (2026-07-13, suite)
- `drive-localization` : 49 cultures × ~1444 clés portées depuis les .resw
  originaux ; en-US statique (build.rs) + autres cultures parsées paresseusement ;
  API tr()/set_culture/detect_system_culture ; 10 tests. Intégré dans drive-app
  (tous les libellés UI passent par tr(), culture système détectée au démarrage).
- Page Paramètres complète : navigation interne (9 sections, libellés resw),
  Apparence avec dropdowns natifs Thème + Arrière-plan (Mica/Mica Alt/Acrylique
  appliqués via DWMWA_SYSTEMBACKDROP_TYPE, persistés), Fichiers et dossiers avec
  ToggleSwitch dessinés.
- Mode superposition compacte (Enter/Exit) : fenêtre topmost 420×400, placement
  restauré à la sortie ; entrée de menu dégrisée.
- Restant prioritaire : « Ajouter un volet » (double volet), vues Grille/Colonnes,
  recherche, renommage inline, tri par en-têtes de colonnes, StatusCenter.
- Flyout Fluent custom (overlay D2D) pour TabActions : Nouvelle fenêtre (Ctrl+N
  câblé), Entrer/Quitter le mode compact, Diviser le volet avec sous-menu
  Vertical/Horizontal (grisés, double volet à porter). Ombre, hover, Échap,
  clic-extérieur. Réutilisable pour les futurs menus.
- Tri par en-têtes de colonnes (Nom/Date/Type/Taille) avec bascule asc/desc,
  chevron accent, dossiers toujours groupés en premier (port de SortOption).
- Double volet (port de ShellPanesPage) : TabGroup 1-2 panes par onglet,
  split Vertical/Horizontal depuis le flyout, navigation indépendante par
  volet, focus au clic (titre d'onglet + breadcrumb + barre d'outils suivent
  le volet actif), volet inactif rendu en lecture seule, colonnes responsives
  (Date/Type/Taille masquées dans les volets étroits), clipping DirectWrite.
- Capture d'écran de vérification passée à PrintWindow(PW_RENDERFULLCONTENT) :
  ne capture plus que le contenu de la fenêtre Files.
- Vue Grille (port de GridLayoutPage) : mode par volet (Details/Grid), tuiles
  110px avec icônes Shell 48px, sélection/hover, réutilise les hit-tests des
  lignes (double-clic, menu contextuel, Suppr fonctionnent tels quels).
  Bouton "Afficher" dans la barre d'outils → flyout Détails/Grille (coche sur
  le mode actif) ; le composant Flyout est devenu générique (FlyoutKind).
- Qualité de rendu : alignement systématique sur la grille de pixels physiques
  (Painter::px/snap) — remplissages et rects de texte arrondis au pixel,
  bordures d'exactement 1 pixel physique centrées sur les centres de pixels
  (fini le halo 1,75px aux échelles DPI fractionnaires), icônes déjà snappées,
  texte en anticrénelage niveaux de gris (correct sur surface transparente).
- Watcher temps réel par dossier (ReadDirectoryChangesW, port de
  WatchForDirectoryChanges) : un watcher par Dir ouvert, debounce 200ms,
  rafraîchissement auto — vérifié avec création de fichier externe.
- Éditeur de texte D2D (EditState : caret, sélection, WM_CHAR, mesures
  DirectWrite) : renommage F2 inline (stem présélectionné, Entrée/Échap/clic,
  IFileOperation — vérifié sur disque) + recherche/filtre en direct de
  l'omnibar (icône loupe → boîte accent, filtrage insensible à la casse).
- StatusCenter : collage Ctrl+V en arrière-plan sur thread STA dédié,
  événements du sink IFileOperation → cartes de progression en bas à droite,
  rafraîchissement final par le watcher.
- Qualité d'icônes (cause racine) : IShellItemImageFactory renvoie de l'alpha
  DROIT malgré la doc ; D2D l'interprétait comme prémultiplié → franges/crénelage.
  premultiply_alpha_in_place dans hbitmap_to_bgra (détection sûre, couvre aussi
  HICON), retry E_PENDING, quantification [16,24,32,48,256] (frame jumbo native,
  une seule passe de resampling). Preuve : invariant prémultiplié 0 violation,
  exemple icon_dump (PNG + histogrammes alpha). 24 tests storage.
- Barre de commandes (port de Toolbar.xaml) : Nouveau ▾ (Dossier/Document
  texte via IFileOperation), Couper/Copier/Coller/Renommer/Supprimer (activés
  selon la sélection), Trier ▾ à droite ; visible sur les vues dossier.
- Barre d'état (port de StatusBar) : « N éléments | sélection ».
- Grille : vraies miniatures Shell (aperçus PDF/images, SIIGBF sans ICONONLY)
  chargées en ASYNCHRONE sur un worker STA (l'extraction peut prendre des
  secondes ; l'icône s'affiche instantanément puis la miniature arrive par
  WM_APP_ICON_READY) ; noms multi-lignes (DWRITE wrap).
- Panneau Détails (port de InfoPane.xaml) : onglets Détails/Aperçu, miniature
  140px async, nom, Nombre d'éléments (dossiers), Date de modification/création
  (format localisé fr), Chemin de l'élément, Étiquettes (stub), bouton
  Propriétés (verbe shell "properties"). Toggle dans la barre de commandes,
  persisté (show_info_pane).
- Bloc droit de la Toolbar (fidèle à Toolbar.xaml) : Filtre (ouvre la boîte de
  filtre), Options de sélection ▾ (Tout sélectionner/Inverser grisés jusqu'à la
  multi-sélection, Effacer fonctionnel), Trier ▾, Grouper ▾ (Aucun coché,
  groupement à porter), toggle volet Détails rempli accent quand actif.
- Icônes vectorielles ORIGINALES : nouveau module vector_icons (parseur de
  tracés SVG → ID2D1PathGeometry, EvenOdd, cache) + assets/themed-icons.txt
  extrait des OutlineIconData de Files.App.Controls (Filter, SelectAll,
  Sorting, Grouping, PanelRight — confirmé par ToggleDetailsPaneAction.cs:22 —
  et Panes.Single pour le bouton TabActions). Le bloc droit de la barre et le
  bouton volets utilisent désormais les tracés exacts de l'original.
  Reste : migrer toutes les autres icônes UI via la même infra (tâche #22).

## RÈGLE DE MÉTHODE (directive utilisateur, 2026-07-13)
Ne RIEN inventer. Chaque composant/fonctionnalité doit être extrait du code
original et reproduit exactement : structure XAML (UserControls/, Views/,
Files.App.Controls/), tracés ThemedIcon (via assets/themed-icons.txt +
vector_icons.rs), clés .resw, comportements des Actions/*.cs. Vérifier par
capture comparée avec l'app originale installée chez l'utilisateur.
Prochain chantier sous cette règle : reproduire l'Omnibar réelle
(Files.App.Controls/Omnibar — modes chemin/palette/recherche, dropdowns par
segment de breadcrumb, bouton ⇥) au lieu de la barre simplifiée actuelle,
puis migrer toutes les icônes restantes (tâche #22) et refaire chaque zone
approximée en la comparant au XAML source.
- Omnibar mode chemin (référence capture utilisateur) : clic dans la barre /
  Ctrl+L / Alt+D → anneau de focus accent double, texte du chemin présélectionné,
  bouton ✕ (efface), panneau de SUGGESTIONS sous la barre (dossier courant +
  sous-dossiers filtrés par préfixe à la frappe, libellés relatifs
  "Parent\Enfant", clic = navigation), Entrée navigue, Échap annule.
  Breadcrumb : chevrons 🏠 > seg > seg > (traînant inclus).
  Reste pour fidélité totale (tâche #23 → #24) : icône de mode à gauche,
  bouton ⇥ et séparateurs internes, dropdowns par segment, mode palette.
- Blocs séparés en CARTES avec marges (référence utilisateur + XAML vérifié :
  ModernShellPage CornerRadius=8, InfoPane CornerRadius=8 + brush dédié,
  MainPage Margin 4,0,0,8) : carte contenu (liste/grille/accueil) et carte
  volet Détails distinctes, gap 8 DIP, bordures, cmdbar et statusbar sur Mica.
- Marges corrigées depuis MainPage.xaml (lecture complète, pas d'estimation) :
  InnerNavigationToolbar Margin 0,0,0,4 ; gap contenu↔InfoPane = GridSplitter
  (4 DIP) ; rangée du bas MinHeight=8 ; marge droite 8. Leçon retenue :
  toujours lire le conteneur XAML entier (Grid.Row/ColumnDefinitions +
  Margins) avant de poser une valeur.
- Barre de commandes en CARTE (Toolbar.xaml : Background
  CardBackgroundFillColorSecondary, BorderThickness 1, CornerRadius 8),
  marges verticales 8/8, boutons 32px inset.
- Gamme typographique alignée sur le XAML : onglets 12 (TabBar.xaml),
  nom du volet Détails 20 (InfoPane.xaml:234), sections 14 semibold,
  corps 14, légendes 12 (caption_strong ajouté pour l'onglet actif).
- InfoPane conforme au XAML (relecture complète) : sélecteur Détails/Aperçu =
  conteneur centré SubtleFill+divider r4, radios 32px, SelectionPill accent
  3px r1 sous l'onglet coché, foreground primary/secondary ; actions
  ToggleDetailsPane/TogglePreviewPane → SelectedTab persisté
  (info_pane_tab). Onglet Aperçu FONCTIONNEL : images/vidéos/PDF/dossiers →
  miniature grand format ; txt/md/html → extrait de texte (4Ko) ;
  sinon DetailsPanePreviewNotAvaliableText. Nom centré Bold 20, marges 12.
  Reste : pilules d'étiquettes, jauge d'espace pour les lecteurs, éditeur de
  tags (EditTags), preview riche (rendu image réelle via IThumbnailProvider).
- Ombre portée des cartes : ThemeShadow (ModernShellPage.xaml:42) approximé
  en D2D (draw_card_shadow : 4 couches noires a=0.05, spread croissant,
  décalage bas) appliqué aux cartes contenu, InfoPane et barre de commandes.

## Session 2026-07-13 (suite) — UserControls, icônes cmdbar, page Apparence

- Restructuration en modules miroirs de `Files.App/UserControls/` (tâche #27,
  agent) : `controls/{navigation_toolbar,toolbar,status_bar,tab_bar,
  info_pane,widgets,file_area,flyout,edit_box,status_center}.rs`, chaque
  module documente son XAML d'origine. ui.rs 2189→~1200 lignes (Painter,
  Rect/Hot/UiState/Layout, sidebar, settings). window.rs inchangé (re-exports
  dans ui.rs). Vérifié : build, 50 tests, capture identique.
- Icônes de la barre de commandes migrées vers les OutlineIconData originaux
  (tâche #22 partielle) : Couper/Copier/Coller/Renommer/Supprimer via
  vector_icon("Cut"/"Copy"/"Paste"/"Rename"/"Delete"), état désactivé
  (TextFillColorSecondary) sans sélection, Coller toujours actif.
- vector_icons : support des arcs SVG `A` (D2D1_ARC_SEGMENT), du préfixe
  XAML `F1` (fill Nonzero) et des viewbox ≠16 (`=== Nom @20` dans
  themed-icons.txt). Ajoutés : TabActions (16), WebAsset/Toolbar/StatusBar
  (@20, extraits de Styles/PathIcons.xaml).
- Page Paramètres > Apparence COMPLÈTE (tâche #28, signalée par capture
  utilisateur) : `views/settings/appearance_page.rs` miroir de
  `AppearancePage.xaml` + AppearanceViewModel :
  * 20 couleurs du fond (AppThemeResourceFactory, hex verbatim #32xxxxxx),
    vignettes 120px avec mini-aperçu (bande couleur 66px r4, onglet 32×12
    LayerOnMicaBaseAlt, zone fichiers, nom centré), sélection = bordure
    accent, clic → app_theme_background_color persisté + teinte plein écran
    (App.Theme.BackgroundBrush) dessinée sous les cartes. Vérifié : clic
    "Bleu" → teinte bleue + pilule déplacée, retour "Par défaut" OK.
  * Expanders BgColor (ouvert par défaut)/BgImage/Barre d'adresse/Barre
    d'outils avec cartes groupées + séparateurs, chevron E70D/E70E.
  * Image de fond : SplitButton Parcourir (IFileOpenDialog filtre images) +
    menu Supprimer ; opacité (slider .1–1 pas .1), ImageFit
    (None/Fill/Uniform/UniformToFill, défaut UniformToFill), alignements
    V/H ; rendu D2D avec clip fenêtre + cache bitmap (invalidé sur device
    loss).
  * Police : énumération DWrite GetSystemFontCollection, "Par défaut" =
    Segoe UI Variable (Constants.Appearance.StandardFont), changement →
    rebuild des TextFormats à chaud.
  * ShowTabActions/ShowToolbar/ShowStatusBar : toggles branchés (bouton
    actions d'onglet masqué, cmdbar/statusbar retirés du layout) ;
    ShowStatusCenterButton (Always/DuringOngoingFileOperations).
  * Défaut backdrop corrigé : MicaAlt (AppearanceSettingsService), et
    layer_fill ajouté au thème (LayerOnMicaBaseAltFillColorDefault
    #B3FFFFFF / #733A3A3A).
  * Titre de section défile avec le contenu (corrigé après capture).
  Reste : ColorPicker personnalisé (flyout du bouton couleur),
  ToolbarCustomizationDialog, couleurs par zone (AddressBar/Toolbar/Sidebar/
  FileArea/InfoPane BackgroundColor).

## Session 2026-07-13 (suite 2) — NavigationToolbar répliqué (bande onglets + barre d'adresse)

Bloc repéré : MainPage.xaml:157 (TabBar) + :165 (NavigationToolbar) ;
UserControls/TabBar/TabBar.xaml et UserControls/NavigationToolbar.xaml
(Grid Height=48, Padding=4,0,4,0, ColumnSpacing=4, colonnes Auto|*|Auto).
- Modes Omnibar répliqués : la loupe séparée est supprimée, les TROIS modes
  (chemin / palette de commandes / recherche) vivent au bord droit DANS la
  barre avec les géométries App.ThemedIcons.Omnibar.{Path,Commands,Search}
  extraites d'Icons.Common.xaml. Mode actif = icône à gauche de la saisie.
  Suggestions dessinées en DERNIER (vrai flyout au-dessus du contenu).
- Palette de commandes (OpenCommandPalette) FONCTIONNELLE : saisie filtrante,
  12 commandes localisées (Refresh/Copy/Cut/Paste/Delete/Rename/NewTab/
  NewWindow/Settings/ToggleDetailsPane/ToggleSidebar/EditPath), Entrée ou
  clic exécute (vérifié : « para » + Entrée → Paramètres ; « lat » →
  masque la sidebar). EDIT_PALETTE = usize::MAX-2.
- Bouton Centre de statut (colonne 2, RightSideActionsStackPanel) : icône
  App.ThemedIcons.StatusCenter (Icons.Misc16.xaml), badge accent pendant les
  opérations (ops_active), flyout 400px MinHeight 120 aligné à droite avec
  état vide « NoFileOperations », light-dismiss. Visibilité pilotée par
  StatusCenterVisibility (Always / DuringOngoingFileOperations).
- SidebarPaneToggleButton : hamburger 36×32 (E700) affiché à gauche de
  Précédent uniquement quand le volet est fermé ; clic → réouverture.
  Vérifié par capture.
- Flyouts d'historique Précédent/Suivant (BackHistoryFlyout &
  ForwardHistoryFlyout) : clic droit → menu des entrées de l'historique
  (Tab::go_to_history ajouté à nav.rs).
- Sélecteur de disposition déplacé À SA PLACE originale : AppBarButton
  LayoutOptionsButton dans Toolbar.xaml entre Groupe et le toggle InfoPane
  (Hot::CmdLayout), icône = IconLayout.Details / IconSize.Small (Grid) selon
  le mode courant, flyout Détails/Grille.
- Reste (fidélité totale du bloc) : dropdowns par segment du fil d'Ariane,
  ProgressRing/InfoBadge réels dans le bouton Centre de statut, drag & drop
  sur les segments, palette riche (icônes + hotkeys dans les suggestions),
  flyout d'historique par appui long.

## Session 2026-07-13 (suite 3) — comparaison avec l'app originale COMPILÉE

MÉTHODE (demande utilisateur, enregistrée en mémoire durable) : lancer
régulièrement `C:\Program Files\WindowsApps\49306atecsolution.FilesUWP_4.2.0.0_
x64__et10x9a9vyk8t\Files.exe` (build du code original) et comparer par
captures. Script `shoot-both.ps1 -PathLike WindowsApps|target` (les deux
processus s'appellent « Files » — filtrer par chemin).
Écarts trouvés et corrigés à la première comparaison :
- Omnibar : l'original n'affiche à droite QUE les modes inactifs (palette,
  recherche) séparés d'un trait — l'icône du mode courant (chemin) n'y est
  pas répétée. Corrigé + divider.
- Nav des Paramètres : icônes par section (App.ThemedIcons.Settings.* ×9
  extraits d'Icons.Settings.Sidebar.xaml), titres « Paramètres »/section en
  20 semibold (title), colonne élargie à 276 (page à +300) — « Outils de
  développement » ne tronque plus.
- Cartes de réglages : hauteur 64 (SettingsCard MinHeight, calibrée sur
  l'app vivante), contrôles centrés verticalement (combo/toggle/slider/
  boutons recalculés depuis le centre).
Écarts restants (tâche #30) : page Général vide chez nous (Langue, Format de
date, Paramètres de démarrage, focus onglet, Widgets, en-têtes de groupe
« Mode double volet » et « Personnaliser le menu contextuel »), sections
sidebar Réseau/Étiquettes + chevrons d'expansion des lecteurs.

### Comparaison visuelle de la barre de navigation (originale vivante vs port)
- Fil d'Ariane : chevron racine ajouté pour Paramètres (« 🏠 > Paramètres »),
  vérifié identique à l'originale par superposition des captures.
- Icônes de navigation ←/→/↑/⟳ : passées de 16 à 14 (FontIcon FontSize="14"
  dans NavigationToolbar.xaml) — nouveau format icon_nav.
- Conformes : hauteur de barre (36), fond/rayon de l'omnibar, position et
  taille du texte, icône Accueil, modes inactifs (palette|recherche) au bord
  droit, bouton Centre de statut.
- Écarts résiduels (<3 DIP) : départ des boutons de nav ~10 px plus à droite
  chez nous, pas de traitement pour l'instant.
- Omnibar : rayon 19 (OmnibarDefaultCornerRadius, pilule) au lieu de 6 ;
  focus accent au même rayon ; hover des modes en pilule
  (OmnibarModeDefaultCornerRadius=17) ; en saisie, plus de boîte interne —
  la barre-pilule EST le champ (draw_edit_text sans chrome, le renommage
  inline garde draw_edit_box).

### TabBar répliqué à l'identique (TabBar.xaml + TabBarStyles.xaml, demande utilisateur)
Mise en forme :
- Bande = TabView Margin 0,10,0,0 + TabViewItemMinHeight 32 → TAB_BAR_HEIGHT 42 ;
  onglets 100–240 de large (clamp WinUI), collés (pas d'espace), départ x=36
  (bouton TabActions 30×30 Margin 4,0,-2,0) ; bouton + 30×30 FontIcon 12.
- Fermeture : bouton 24×24 r4, glyphe E711 taille 12, marge droite 4.
- BottomBorderLine 1 DIP sous la bande, interrompue par l'onglet actif ;
  arcs concaves Left/RightRadiusRenderArc reproduits en géométries
  vectorielles (TabArcLeft/TabArcRight @4 dans themed-icons.txt) aux pieds
  de l'onglet actif. Actif = fond opaque fusionné + label semibold.
Fonctionnalités :
- Menu contextuel d'onglet (TabFlyout) : Nouvel onglet, Dupliquer l'onglet,
  Déplacer vers une nouvelle fenêtre, Fermer à gauche/à droite/les autres
  (grisés selon la position, dropdown_ex MF_GRAYED), Rouvrir l'onglet fermé
  (pile closed_tabs alimentée par toutes les fermetures).
- Clic molette sur un onglet = fermer (vérifié par test synthétique).
- Molette sur la bande = onglet suivant/précédent (TabView_PointerWheelChanged).
- Ctrl+Tab / Ctrl+Maj+Tab = cycle des onglets ; Ctrl+W → close_tab (pile).
- Glisser un onglet hors de la bande (>48 DIP) = déplacé vers une NOUVELLE
  FENÊTRE (TabDroppedOutside), comme « Déplacer vers une nouvelle fenêtre »
  (spawn de l'exe sur le chemin + retrait local).
Reste : drag & drop de FICHIERS sur un onglet/le bouton + (OLE), tooltip
d'onglet, réordonnancement des fenêtres duplicées avec double volet.

## Session 2026-07-14 — les 8 pages Paramètres restantes (tâche #30)

Modèle générique `views/settings/controls.rs` (SettingsCard / SettingsExpander /
en-têtes de groupe / InfoBar / panneaux custom) : `SettingsRow { id: SettingId,
kind, icon (Glyph|Vector|VectorColored), label, description, control }`,
contrôles Combo (largeur ajustée au texte)/Toggle (+variantes grisées)/Action
(E8A7)/Button/Buttons/Text/Shortcut (pastilles de raccourci), calcul de layout
et dessin génériques, cartes d'expander groupées avec séparateurs comme
Apparence. `UiState.settings_expanded[[bool;4];9]` (défauts IsExpanded du
XAML : Fichiers-et-dossiers>CalculateFolderSizes et Étiquettes ouverts).
`ui.rs` : dispatch générique via `Layout.settings_page` (l'ancien modèle
simpliste supprimé, section Apparence inchangée). `window.rs` :
`on_settings_row_id` (match SettingId), `dropdown_checked_ex` (coché+grisé),
pick_folder / pick_open_json / pick_save_json, windows_version (registre),
`file_ops::clipboard_set_text` (CF_UNICODETEXT).
- **Général** (GeneralPage.xaml) : Langue (GetLocaleInfoEx, suit le système,
  lecture seule), Format de date avec exemple (`DateFormatSample` +
  formatters Application/System/Universal portés dans `datetime.rs`,
  branchés sur la colonne « Modifié le » via `modified_text`), Paramètres de
  démarrage (combo 3 choix + panneau « pages au démarrage » fonctionnel :
  Ajouter Accueil/Parcourir, suppression, persisté ; OpenTabInExistingInstance),
  focus nouvel onglet, Widgets (5 toggles), groupes « Mode double volet »
  (toggle + direction Vertical/Horizontal), « Personnaliser le menu contextuel »
  (expander 13 toggles + débordement sous-menu), « Défilement » (fluide +
  direction molette onglets). Défauts = GeneralSettingsService.
- **Disposition** (LayoutPage.xaml) : Sync préférences (bascule Adaptatif→
  Détails comme le ViewModel), Type de disposition (6 choix, Adaptatif grisé
  si sync), Trier par (+ ordre décroissant, priorité de tri), Grouper par
  (+ décroissant/unité de date grisés selon l'état), Vue détaillée
  (ajustement auto — clé en fallback en-US comme l'original — + colonnes
  Étiquette/Taille/Type/Date/Date de création).
- **Fichiers et dossiers** (FoldersPage.xaml) : Affichage (Éléments masqués
  → RÉUTILISE show_hidden_items avec refresh, fichiers commençant par un
  point, fichiers système, flux alternatifs), extensions (RÉUTILISE
  show_file_extensions), miniatures (branché sur les vignettes de la vue
  Grille), cases à cocher ; Comportements (simple clic ×3, nouvel onglet,
  confirmation de suppression, avertissement d'extension, sélection au
  survol, double-clic remonter, défilement vers le dossier précédent,
  format de taille Binaire/Décimal, calcul des tailles + InfoBar
  d'avertissement).
- **Actions** (ActionsPage.xaml) : barre Commandes (recherche + Ajouter +
  Rétablir, inertes) et les 72 raccourcis PAR DÉFAUT extraits de
  `Files.App/Actions/**` (ordre ordinal CommandManager, HotKey/SecondHotKey…,
  sans les bindings invisibles/souris ni les commandes non globales),
  descriptions localisées avec résolution du pluriel ICU, pastilles
  Ctrl/Maj/Alt (clés Key.* du resw, touches OEM via MapVirtualKeyW).
- **Étiquettes** (TagsPage.xaml) : expander FileTags ouvert, bouton
  « Nouvelle étiquette » (inerte), les 4 étiquettes par défaut du
  FileTagsSettingsService (Home #0072BD, Work #D95319, Photos #EDB120,
  Important #77AC30) avec pastille PathIcon.FilledTag colorée et boutons
  Modifier/Supprimer (inertes) ; persistées dans settings.json.
- **Outils de développement** (DevToolsPage.xaml) : bouton « Ouvrir l'IDE »
  (combo Repos Git/Tous les emplacements), Nom/Chemin de l'IDE en lecture
  (défaut VS Code si `code` dans PATH), Se connecter à GitHub (inerte).
- **Avancé** (AdvancedPage.xaml) : Exporter/Importer les paramètres
  (IFileSaveDialog/IFileOpenDialog JSON, FONCTIONNELS sur notre
  settings.json), Modifier le fichier des paramètres (ouvre le json),
  démarrage Windows / app en arrière-plan / icône de zone de notification /
  explorateur par défaut / options d'aplatissement (persistés, inertes) ;
  les blocs dev-only (dialogue Ouvrir, cache des miniatures) masqués comme
  en release.
- **À propos** (AboutPage.xaml) : carte « Files / Version 4.2.0.0 » + bouton
  Copier (menu Version de l'application / Version de Windows / ID
  utilisateur → presse-papiers, versions réelles, user_id GUID persisté),
  Soutenez-nous, Documentation, Questions et discussions, Commentaires
  (demande de fonctionnalité / rapport de bug avec query versions), journal
  (%LOCALAPPDATA%\FilesRust), Traduire, Bibliothèques tierces (grille de
  21 liens cliquables, données AboutViewModel verbatim), Dépôt GitHub,
  Confidentialité — URLs Constants.ExternalUrl verbatim.
- Icônes vectorielles ajoutées à themed-icons.txt : Hide @20 (PathIcons.xaml),
  FilledTag, OpenFoldersNewTab (path inline de FoldersPage.xaml).
- Settings : ~70 réglages ajoutés avec les défauts EXACTS des services
  (date_time_format Application, continue_last_session true,
  open_tab_in_existing_instance true, show_file_tags_widget false,
  default_layout_mode Adaptive, single-click OnlyForTouch/Always,
  size_unit BinaryUnits, leave_app_running true (RELEASE), etc.).
- Vérifié : cargo build OK, 52 tests (50 + 2 datetime), captures des 9
  sections + expanders (démarrage, widgets, menu contextuel, IDE,
  commentaires, bibliothèques) — rien ne déborde après élargissement
  dynamique des combos ; toggle « Afficher les miniatures » vérifié
  aller-retour dans settings.json ; Apparence sans régression.
- Reste : édition des raccourcis (ActionsSettingsService), éditeur
  d'étiquettes (ColorPicker), édition IDE + test d'intégration, OAuth
  GitHub, vraie tâche de démarrage Windows/tray, wrap des libellés longs
  sur fenêtres étroites (l'original replie le texte).

### Audit des ressources de style (App.xaml + styles de contrôles) — suite à question utilisateur
La comparaison initiale couvrait TabBar.xaml/TabBarStyles.xaml mais pas les
ThemeResource : App.xaml surcharge les couleurs PAR ZONE. Corrections :
- TabViewItemHeaderBackgroundSelected = App.Theme.AddressBar.BackgroundBrush
  = LayerOnMicaBaseAltFillColorDefault → tab_active_background redevient
  TRANSLUCIDE (#B3FFFFFF / #733A3A3A) ; la bande d'outils partage la même
  couche → fusion sans couture au-dessus du backdrop (l'opacité forcée
  d'antan n'est plus nécessaire).
- App.Theme.Sidebar.BackgroundBrush = LayerOnMicaBaseAlt → fond de sidebar
  désormais dessiné (il manquait).
- App.Theme.FileArea.BackgroundBrush : clair #C0FCFCFC, sombre
  CardBackgroundFillColorDefault #0DFFFFFF → layer_background corrigé.
- App.Theme.InfoPane.BackgroundBrush = CardBackgroundFillColorSecondary →
  volet Détails aligné sur toolbar_background.
- Omnibar.xaml (Files.App.Controls) : Height=38 (au lieu de 36), Background
  ControlFillColorDefault, bord 1 (2 focus), boutons de nav 36×32 centrés
  dans la rangée de 48.
Restent des ThemeResource WinUI génériques non vérifiables hors dépôt
(TabViewItemHeaderPadding…) — calibrés sur l'app vivante.
- Pieds de l'onglet actif (retour utilisateur « pas des angles droits ») :
  les DEUX éléments du template sont désormais reproduits — (1) le flare de
  fond (TabArcLeft/Right, quart de cercle concave rempli du fond d'onglet
  actif) et (2) les CROISSANTS de bordure 1px verbatim du XAML
  (TabBorderArcLeft/Right = « M4 0C4 1.19469… », Fill=TabViewBorderBrush ≈
  divider) qui font tourner la BottomBorderLine autour du pied de l'onglet ;
  la ligne s'arrête 4 DIP avant chaque bord. Vérifié par zoom ×6 : l'arc se
  rendait déjà (test en accent) mais était subtil — c'est le croissant de
  bordure qui manquait.
- Onglet actif : le fond n'est plus « rectangle arrondi + arcs collés »
  (couture visible au zoom) mais UNE SEULE géométrie D2D reconstruisant
  TabViewTemplateSettings.TabGeometry : coins hauts r8 (OverlayCornerRadius),
  murs droits, pieds évasés concaves r4 — fill_tab_shape() dans tab_bar.rs.
  Les croissants de bordure 1px restent superposés. Zoom ×10 comparé à
  l'originale : raccord courbe identique, sans couture.
- « Bande sombre autour de l'onglet actif » (retour utilisateur) : le tracé
  était propre (grille de pixels vérifiée, aucun pixel < fond) — le coupable
  était le fond de bande de titre rgb(28,28,28) OPAQUE peint en thème
  sombre, hérité d'une calibration ancienne. TabBar.xaml déclare
  TitlebarArea Background="Transparent" : la bande laisse voir le Mica.
  Corrigé (alpha 0 en sombre ; la teinte accent reste réservée au thème
  clair avec ColorPrevalence). ATTENTION VÉRIF : PrintWindow ne composite
  pas le backdrop DWM → les zones transparentes paraissent noires en
  capture ; seul l'écran réel fait foi pour ce point.
- Halo fin persistant autour de l'onglet actif : causé par les DEMI-PIXELS —
  top à 17,5 px phys (10 DIP × 1,75) et bas à 73,5 pile sur la limite du
  clip → rangées à 50 % de couverture laissant transparaître le fond.
  Corrigé : fill_tab_shape snappe ses bornes sur la grille de pixels
  (Painter::px rendu pub(crate)) et la forme est dessinée HORS du clip de
  bande pour que son bas snappé tombe sur la même rangée device que le fill
  du strip. Vérifié au pixel : transition en 1 rangée en haut, aucune
  couture en bas (41,44,47 continu).
- Continuité onglet actif ↔ bande d'outils (retour utilisateur) : deux
  corrections d'ordre de dessin fidèles au template — (1) les croissants de
  bordure sont dessinés AVANT le fond sélectionné qui les recouvre (z-order
  du XAML : BottomBorderLine → arcs → SelectedBackgroundPath), seule leur
  frange extérieure transparaît ; (2) la BottomBorderLine n'est plus
  raccourcie de 4 DIP : dans l'original chaque onglet rend SON segment (
  masqué sur l'actif), la ligne des voisins atteint donc exactement les murs
  de l'onglet actif et le flare translucide passe par-dessus son extrémité,
  la fondant dans la courbe. Vérifié au zoom ×12 : plus d'encoche sombre au
  pied, la ligne s'estompe dans le raccord.
- Anticrénelage des arcs de raccord (retour utilisateur) : la jonction
  empilait 3 remplissages translucides à bords AA distincts (flare, strip,
  croissants) → moiré/crénelage. Refonte : draw_toolbar dessine désormais
  la bande d'outils ET la silhouette de l'onglet actif comme UNE SEULE
  géométrie D2D en mode WINDING (add_tab_figure ajoute la figure de
  l'onglet avec une jupe de 2 DIP dans la figure du strip → union sans
  arête interne, un seul bord AA analytique). Ordre de dessin inversé
  (toolbar avant tab_bar) ; le fond accent de bande (thème clair) est peint
  par draw_toolbar sous l'union ; croissants toujours dessous. Zoom ×12 :
  dégradé AA multi-niveaux fluide, plus de marches.
- Décalage horizontal d'1 px des croissants (retour utilisateur) : les deux
  chemins de snapping divergeaient — vector_icon arrondit depuis le CENTRE
  de sa boîte, add_tab_figure depuis ses BORDS ; pour un tab.left
  fractionnaire les arrondis diffèrent d'½ DIP (≈1 px phys). Corrigé : les
  boîtes des croissants sont construites depuis les mêmes valeurs
  pixel-snappées (px(left/right/bottom)) que la figure de l'onglet — même
  cercle garanti. Diagnostic utile : des marqueurs debug ont montré que le
  rect de capture PrintWindow inclut ~12 px de bordures invisibles → ne
  jamais se fier aux coordonnées absolues d'une capture, poser des
  marqueurs.
- Débordements aux pieds de l'onglet actif (retour utilisateur, photo) :
  premier diagnostic ERRONÉ — j'avais conclu que les croissants de bordure
  (Left/RightRadiusRenderArc) ne devaient pas être dessinés et je les avais
  supprimés, ce qui a cassé les arcs de raccord (plus de continuité entre la
  ligne de bordure et la courbe du pied).
- Vrai diagnostic (après relecture de l'implémentation originale, sur
  suggestion de l'utilisateur) : le problème n'était pas la présence des
  croissants mais leur PINCEAU. TabBarStyles.xaml peint BottomBorderLine et
  les deux croissants avec `TabViewBorderBrush`, que Files ne redéfinit pas :
  c'est le `CardStrokeColorDefault` de WinUI, soit #0F000000 (clair) et
  #19000000 (sombre) — du NOIR dans les deux thèmes. Nous utilisions
  `divider` (blanc 8 % en sombre) → d'où les bosses claires. Corrigé :
  nouveau token `theme.tab_border`, utilisé par la ligne du bas ET les
  croissants (géométries verbatim TabBorderArcLeft/Right). Au passage
  `tab_separator` (TabViewItemSeparator = DividerStrokeColorDefault) passe de
  0,25 d'alpha aux vraies valeurs #0F000000 / #15FFFFFF.
- Géométrie du croissant (vérifiée mathématiquement contre la data XAML) :
  bande entre les rayons 3 et 4 du cercle centré sur (tab.left-4, bas-4),
  coupée à y=3 — exactement le prolongement courbe de la ligne de 1 px. Le
  point (2.64582, 3) de la courbe est l'abscisse √7 où l'arc de rayon 4 croise
  y=3, c.-à-d. là où le remplissage de l'onglet reprend le relais.
## Boutons de légende (réduire / agrandir / fermer) : au SYSTÈME

Glyphes dédoublés et décalés (retour utilisateur, capture). Cause : nous les
dessinions nous-mêmes ALORS QUE le système les dessinait déjà — deux jeux
superposés, le nôtre centré sur 42 DIP, celui de DWM sur 32.

L'original n'en dessine aucun : `MainWindow.xaml.cs` se contente de
`ExtendsContentIntoTitleBar = true` puis met à Transparent les quatre couleurs
de FOND des boutons (`AppWindow.TitleBar.Button*BackgroundColor`). Les boutons
sont donc rendus et pilotés par le système. `TabBar.xaml.cs` le confirme :
`RightPaddingColumn` réserve `TitleBar.RightInset / scale + 40`.

Port : dessin supprimé, et `DwmDefWindowProc` appelé EN TÊTE du wndproc — c'est
lui qui donne aux boutons système leur hit-testing, leur survol et leurs clics.
Le layout ne fait plus que réserver leur largeur.

## Réorganisation des onglets : animation (ListView reorder)

L'original ne code pas l'animation : elle vient de la machinerie ListView du
TabView, et `TabBarStyles.xaml` en fixe les paramètres via ses VisualStates.
Valeurs relevées à la source (`ListViewItem_themeresources.xaml` de WinUI) :

- `ListViewItemDragThemeOpacity` = **0.80** → l'onglet glissé est peint
  translucide (état `Dragging` : `DoubleAnimation` sur `LayoutRoot.Opacity`).
- `ListViewItemReorderThemeOpacity` = 0.80, `…ReorderTargetThemeOpacity` = 0.50,
  `ListViewItemReorderHintThemeOffset` = 10.0 (états `ReorderHintStates`).
- Durée du retour : `<VisualTransition GeneratedDuration="0:0:0.2" />` → **0,2 s**,
  sur une courbe décélérante (ease-out cubique).

Port : `TabSlide` dans `UiState` (offset restant par onglet, décroissant sur
0,2 s), `tab_drag_offset` pour l'onglet suivi par le curseur. À chaque
permutation, les onglets enjambés repartent de ±largeur et glissent vers leur
nouveau slot ; au relâchement l'onglet glissé rejoint son slot au lieu de sauter.
Timer dédié (id 43) à 16 ms, arrêté dès que tout est stabilisé. L'onglet glissé
est peint EN DERNIER (TabView relève son `Canvas.ZIndex`) et, s'il est l'onglet
actif, il est retiré de l'union avec la bande de la barre d'adresse : il est
détaché, il ne doit donc plus l'entailler.

## Menu contextuel des onglets (TabFlyout) : style Fluent, pas un popup Win32

Le `TabFlyout` de l'original est un `MenuFlyout` WinUI. Nous l'affichions avec
un `TrackPopupMenuEx` Win32 : le contenu était bon, l'apparence n'avait rien à
voir (menu système gris, pas de coins arrondis, pas de colonne d'icônes).
Corrigé : il passe désormais par notre flyout Fluent (`controls/flyout.rs`,
porté de `MenuFlyoutItemWithThemedIcon`), déjà utilisé par les menus déroulants
— panneau arrondi (rayon 8), ombre portée, survol arrondi, colonne d'icônes,
raccourcis alignés à droite, entrées désactivées en TextFillColorSecondary.
`FlyoutKind::TabContext(i)` porte l'onglet visé ; `run_tab_context_command`
exécute la commande. La largeur n'est plus une constante : elle est calculée sur
le contenu (un MenuFlyout s'adapte à ses entrées), sinon « Déplacer l'onglet
vers une nouvelle fenêtre » était tronqué.

RESTE À FAIRE : le menu contextuel des fichiers passe encore par le menu shell
natif (`IContextMenu`). L'original construit le sien (`ItemContextMenu`) à
partir de ses propres commandes, et ne délègue au shell que via « Afficher plus
d'options ». C'est le prochain gros morceau.

7 entrées, dans l'ordre de `TabBar.xaml`. Deux écarts corrigés :
- `TabItemContextMenu_Opening` fait `MenuItemMoveTabToNewWindow.IsEnabled =
  Items.Count > 1` — nous l'affichions toujours actif.
- Chaque entrée porte le `KeyboardAcceleratorTextOverride` de sa commande :
  NewTab = Ctrl+T, DuplicateTab = Ctrl+Maj+K, ReopenClosedTab = Ctrl+Maj+T (les
  trois « Fermer… » n'ont pas de raccourci). Comme `HotKey.LocalizedLabel` de
  l'original, le texte est construit à partir des noms de touches du clavier
  (`GetKeyNameTextW`) : « Maj » en français, pas « Shift ».
- Reste à faire : l'icône FontIcon E8A7 de « Déplacer vers une nouvelle
  fenêtre » (un menu Win32 exige un HBITMAP par entrée).

## Onglets inactifs : structure et marges (TabBarItemStyle + WinUI TabView)

Files ne redéfinit AUCUNE des métriques de TabView (seulement des pinceaux
dans App.xaml) : elles viennent de `controls/dev/TabView/TabView_themeresources.xaml`
de microsoft-ui-xaml, dont les valeurs ont été relues à la source :

- `TabViewItemHeaderPadding` 8,3,4,3 — et pour l'onglet sélectionné
  `TabViewSelectedItemHeaderMargin` -1,0,-1,1 avec `TabViewSelectedItemHeaderPadding`
  9,3,5,4. Les deux retombent sur la MÊME boîte de contenu (le +1 de padding
  compense le -1 de marge) : rien ne bouge quand un onglet devient actif.
  D'où `tab_content_rect()` unique.
- Icône 16×16 (`TabViewItemHeaderIconSize`) avec `TabViewItemHeaderIconMargin`
  0,0,10,0 ; titre à 12 px (`TabViewItemHeaderFontSize`), SemiBold seulement
  quand sélectionné ; CloseButton 32×24 (et non 24×24) collé au bord droit du
  contenu, `TabViewItemHeaderCloseMargin` 4,0,0,0 avant lui, rayon 4.
- `TabViewItemMinWidth` 100 / `TabViewItemMaxWidth` 240, hauteur 32
  (`TabViewItemMinHeight`) sous la marge haute de 10 du TabView → la bande fait
  bien 42.
- Fond au repos : `SubtleFillColorTransparent` → un onglet inactif ne peint
  RIEN. Au survol : `TabViewItemHeaderBackgroundPointerOver`, que App.xaml
  redéfinit en `SubtleFillColorSecondary` (#09000000 / #0FFFFFFF) → nouveau
  token `tab_hover_background`. Coins HAUTS seulement (rayon 8, via le
  TopCornerRadiusFilterConverter du template) → nouveau `fill_top_rounded()`.
- Bordure de l'onglet : `TabViewItemBorderBrush` = `SubtleFillColorTransparent`
  → invisible, on ne dessine rien (vérifié à la source, pas supposé).
- Texte et icône : `TabViewItemHeaderForeground` = TextFillColorSecondary pour
  un onglet inactif, Primary pour le sélectionné. Nous mettions tout en primary.
- **Colonnes latérales de 4 DIP (marge autour de l'onglet actif).** Le
  `LayoutRoot` du template a trois colonnes : LeftColumn (Auto, qui héberge
  `LeftRadiusRenderArc`, Width=4), la colonne étoile qui porte le
  `TabContainer`, et RightColumn (idem à droite). Le TabContainer — donc le
  FOND DE SURVOL et tout le contenu — est ainsi rentré de 4 DIP de chaque côté
  des bords de l'onglet, alors que la TabGeometry de l'onglet SÉLECTIONNÉ, elle,
  va jusqu'au bord. D'où la marge sombre visible entre l'onglet actif et un
  voisin survolé (signalée par l'utilisateur, capture à l'appui).
  Confirmé par deux mesures indépendantes sur l'app compilée : le titre du
  voisin tombe à +38 DIP du bord d'onglet (= 4 + 8 padding + 16 icône + 10 de
  marge d'icône) et la boîte d'icône à +12 — nous étions à +34 et +8.
  `tab_content_rect` = TabContainer + `TabViewItemHeaderPadding` (8,3,4,3),
  appliqué tel quel. (J'avais d'abord mis le padding droit à 0, sur la foi
  d'une mesure de glyphe faite à l'œil sur un zoom : le bouton de fermeture
  venait alors buter dans le coin arrondi du fond de survol. Leçon : le
  template prime sur une mesure approximative.)
  Note : ce détail est INVISIBLE au repos (fond transparent) et invisible aussi
  dans une capture PrintWindow, car le fond de survol (SubtleFillColorSecondary,
  6 % de blanc) n'est pas composité sur le backdrop Mica — il ne se voit qu'à
  l'écran. Ne pas conclure de son absence dans une capture qu'il n'existe pas.
- `TabSeparator` : 1 px au bord DROIT de l'onglet, `TabViewItemSeparatorMargin`
  0,8,0,8. TabViewItem.cpp (`HideLeftAdjacentTabSeparator`) le masque sur
  l'onglet sélectionné, sur celui juste à sa GAUCHE, et autour de l'onglet
  survolé — jamais de séparateur au contact d'un onglet mis en avant.
- Bord gauche de la bande : 39 DIP, pas 32 (= 4 + 30 − 2 du TabStripHeader).
  L'écart de 7 DIP vient de l'hôte de défilement du TabView et n'apparaît pas
  dans TabBarStyles.xaml ; il est MESURÉ sur l'app compilée (le mur de l'onglet
  sélectionné et l'icône du premier onglet tombent tous deux sur 39).

Vérifications au pixel contre l'app originale : séparateur rendu à rgb(50,50,50)
sur fond (32,32,32) — exactement la valeur de l'originale, ce qui confirme au
passage `TabViewItemSeparator` = `DividerStrokeColorDefault` #15FFFFFF
(32 + 0,082 × (255−32) = 50).

Piège d'outillage : un script PowerShell non DPI-aware qui envoie des clics par
SendMessage voit ses coordonnées multipliées par le facteur d'échelle (1,75)
avant d'atteindre une fenêtre DPI-aware — les clics tombaient hors de la bande
d'onglets et j'ai cru un moment à un bug de sélection. Appeler
`SetProcessDPIAware()` dans le script (fait dans click.ps1 / shoot-both.ps1).

- Ordre de rendu : ligne du bas → croissants → remplissage TabGeometry, le
  tout dans `draw_toolbar`. C'est l'ordre du template, sachant que TabView
  relève le `Canvas.ZIndex` de l'onglet sélectionné : sa géométrie (qui
  déborde de 4 px de chaque côté) passe par-dessus la ligne des voisins.
  Les boîtes 4×4 des pieds viennent de `tab_foot_boxes()`, qui repart des
  mêmes valeurs px()-snappées que `add_tab_figure` (sinon décalage d'1 px).

## Menus contextuels : vraie fenêtre popup acrylique (DWM) et menus manquants

### 1. Une VRAIE fenêtre popup, pas un dessin en-fenêtre

**Mauvaise première approche (abandonnée).** J'avais d'abord dessiné le
menu *dans* la fenêtre en D2D, avec un flou fait main : relecture de la
frame (`CopyFromRenderTarget`), flou gaussien, ombre gaussienne mise en
cache. Deux défauts rédhibitoires que l'utilisateur a pointés : le menu
restait **prisonnier** de la fenêtre (impossible de déborder à droite ou
en bas comme l'original), et le flou d'un fond Mica translucide ne pouvait
pas donner le bon rendu (l'image floutée était quasi transparente, le
texte net d'origine transparaissait).

**Bonne approche.** Dans l'original chaque flyout vit dans sa propre
fenêtre popup (`Microsoft.UI.Content.PopupWindowSiteBridge`) portant un
*system backdrop* DesktopAcrylic. On reproduit ça littéralement —
`controls/flyout_window.rs` :

- fenêtre `WS_POPUP` nue, `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW |
  WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP`, avec son propre swapchain
  DirectComposition (comme la fenêtre principale) ;
- **acrylique via `SetWindowCompositionAttribute` /
  `ACCENT_ENABLE_ACRYLICBLURBEHIND`** ;
- `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND` (= `OverlayCornerRadius`) ;
  l'ombre portée est celle que DWM donne à toute fenêtre popup.

Le contenu est peint sur une cible **transparente** (`Painter::draw_menu`
depuis l'origine 0,0) ; le compositeur fournit le reste.

**Deux impasses, toutes deux vécues — c'est là que le flou se perdait :**

- `DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_TRANSIENTWINDOW` renvoie bien `Ok`,
  mais un *system backdrop* DWM **ne floute que tant que sa fenêtre est
  ACTIVE**. Un menu ne prend jamais le focus : il affichait donc à vie la
  couleur de repli, un gris plat. (C'est aussi pour ça que les menus de
  Windows lui-même ne l'utilisent pas.)
- `DwmExtendFrameIntoClientArea(-1)` (+ le `WS_CAPTION`/`WS_THICKFRAME` que
  le backdrop DWM réclame) fait peindre par DWM le cadre étendu en **voile
  opaque derrière notre contenu** : n'importe quel flou dessous devient
  invisible. Il faut une popup **sans cadre du tout**.

**Entrées & modalité.** Le popup est passif : la fenêtre principale garde
le `SetCapture`, reçoit donc tous les `WM_MOUSEMOVE`/clics (même au-dessus
du popup, même hors de l'appli) et fait tout le hit-test en DIP client — ce
qui marche même pour des points hors fenêtre puisque les coordonnées client
ne sont pas bornées. `sync_flyout` (appelé depuis `render`) réconcilie les
popups avec `state.flyout` : création, placement **clampé à la zone de
travail du moniteur** (d'où le débordement hors appli), repeinture au
survol, ouverture du sous-menu (popup #1). Un `WM_CAPTURECHANGED` ferme le
menu (clic dans une autre appli).

Valeurs du template (inchangées) : bordure `SurfaceStrokeColorFlyoutBrush`,
rayon 8, séparateur `-4,1,-4,1`, item en état souris (NarrowPadding)
11,4,11,5 + marge 4,2,4,2 → ligne de 33 DIP.

### 3. Les menus qui manquaient

Portés depuis `Data/Factories/ContentPageContextFlyoutFactory.cs` :

- **menu d'élément** (`GetItemContextCommandsWithoutShellItems`) — clic
  droit sur une ligne de fichier, une carte d'accès rapide ou de lecteur ;
- **menu d'espace vide** — le même factory avec `itemsSelected == false` :
  Disposition ▸, Trier par ▸, Rafraîchir, Nouveau ▸, Coller, Terminal ;
- **menu de la barre latérale** (SidebarView).

Tous les trois passent par le flyout Fluent (fini le menu Win32 natif),
et se terminent par **« Afficher plus d'options »** (glyphe ``) —
l'entrée `ItemOverflow` de l'original, que Files remplit avec les
extensions shell ; chez nous elle ouvre l'`IContextMenu` du shell.

Conséquences sur `FlyoutItem` :

- `separator` (`ContextMenuFlyoutItemType.Separator`), hauteur de ligne 9
  au lieu de 40 ;
- `children` : le sous-menu appartient à l'item, comme
  `ContextMenuFlyoutItemViewModel.Items` (`Flyout::submenu` n'est plus
  qu'un index) ;
- `checked` (`IsToggle`) → coche `` dans la colonne d'icône ;
- `command: MenuCommand` : le dispatch se fait par **commande**, plus par
  index — le contenu d'un menu contextuel varie avec l'élément visé.

Cinq géométries ThemedIcon extraites en plus (`assets/themed-icons.txt`) :
`OpenFile`, `OpenInWindow`, `OpenInTab`, `CopyAsPath`, `NewItem`.

### Piège : `GetKeyNameTextW` et le bloc de navigation

`MapVirtualKeyW(VK_DELETE, MAPVK_VK_TO_VSC)` renvoie un scan code partagé
avec le pavé numérique. Sans le bit « touche étendue » (1 << 24) dans le
lparam, l'accélérateur de Supprimer s'affiche « . (pavé num.) » au lieu
de « Suppr ».

### 5. La bande d'icônes en haut du menu : `CommandBarFlyout.PrimaryCommands`

Les menus contextuels d'éléments et de zone vide ne sont pas des `MenuFlyout`
mais des **`CommandBarFlyout`** (`BaseLayoutPage.cs` :
`ItemContextMenuFlyout` / `BaseContextMenuFlyout`).
`ContextFlyoutModelToElementHelper.GetAppBarItemsFromModel(items)` répartit les
entrées du modèle en `(primaryElements, secondaryElements)` selon leur drapeau
`IsPrimary` : les premières deviennent des `AppBarButton` sans libellé dans la
bande du haut, les secondes les lignes du menu.

Dans `ContentPageContextFlyoutFactory.GetBaseItemMenuItems`, les entrées
`IsPrimary = true` sont, dans cet ordre :

| commande | condition d'affichage |
|---|---|
| `CutItem` | défaut → `IsExecutable` |
| `CopyItem` | défaut → `IsExecutable` |
| `PasteItemToSelection` | `IsVisible = true` — **toujours** affichée, grisée si le presse-papiers est vide |
| `Rename` | `IsVisible = itemsSelected` |
| `ShareItem` | défaut → `IsExecutable`, et `ShareItemHelpers.IsItemShareable` refuse les dossiers |
| `DeleteItem` | `IsVisible = itemsSelected` |
| `OpenProperties` | `IsVisible = OpenProperties.IsExecutable` |

`ContextMenuFlyoutItemViewModelBuilder.Build()` masque une entrée quand
`IsVisible is null && !isExecutable` : sans sélection (menu de zone vide) il ne
reste donc que **Coller** et **Propriétés**.

**Géométrie, relevée sur l'exécutable original** (menu ouvert, capture de la
région des deux popups, écran à 175 %) : la barre de commandes est un popup
`PopupWindowSiteBridge` distinct, **de la même largeur** que le menu et posé
juste au-dessus, bord à bord (aucun décalage horizontal, aucun espace) — les
deux ne se lisent que comme une seule surface coupée par un filet de 1 px.
Le panneau de la barre mesure **48 DIP** (4 de marge + un `AppBarButton` de 40
+ 4). Les boutons ne s'étirent pas : ils font 40×40 et sont **alignés à
gauche**, si bien qu'un menu large laisse du vide à droite de la barre.

On la dessine donc dans le même popup que le menu (`Painter::draw_menu`), la
bande d'abord, puis le filet, puis les lignes : rendu identique, un popup de
moins à gérer.

`ShareItem` est porté tel quel (`share.rs`) : `IDataTransferManagerInterop`
(`GetForWindow` sur notre HWND, handler `DataRequested` qui remplit le
`DataPackage`, puis `ShowShareUIForWindow`), exactement la route de
`ShareItemHelpers.ShareItemsAsync`.

### 6. Le menu d'élément au complet : port de `GetBaseItemMenuItems`

La liste secondaire est désormais le port ligne à ligne de la factory (avec les
valeurs par défaut de `GeneralSettingsService` : tout à `true` sauf
`ShowCreateAlternateDataStream`), dans l'ordre exact du C# :

| entrée | condition (locale C#) |
|---|---|
| Ouvrir | toujours |
| Ouvrir avec | `showOpenItemWith` : fichier simple, ni raccourci ni exécutable |
| Ouvrir l'emplacement du fichier | l'élément est un `.lnk` / `.url` |
| Ouvrir dans un nouvel onglet / une nouvelle fenêtre | dossier |
| Ouvrir dans un nouveau volet ▸ (vertical / horizontal) | `areAllItemsFolders` |
| Exécuter en tant qu'administrateur / autre utilisateur | `isFirstFileExecutable` |
| — séparateur — | `itemsSelected` |
| Coller le raccourci | grisé si le presse-papiers est vide |
| Copier le chemin de l'élément (Ctrl+Maj+C) | `ShowCopyPath` |
| Créer un dossier avec la sélection | `ShowCreateFolderWithSelection` |
| Créer un raccourci | `ShowCreateShortcut` et pas déjà un raccourci |
| Épingler / Désépingler de la barre latérale | dossier |
| Compresser ▸ (Créer une archive / `<nom>.zip` / `<nom>.7z`) | `canCompress = !canDecompress` |
| Extraire ▸ (Extraire les fichiers / Extraire ici / Extraire vers `<nom>`) | `canDecompress` |
| Aplatir le dossier | dossier |
| Envoyer vers ▸ | `ShowSendToMenu` |
| Modifier dans Bloc-notes | `.bat` / `.cmd` / `.ahk` (c'est bien la condition du C#) |
| — séparateur — + Ouvrir dans le Terminal | dossier |
| — `OverflowSeparator` — + Afficher plus d'options ▸ | toujours |

Couper / Copier / Coller / Renommer / Partager / Supprimer / Propriétés ne
figurent PAS dans cette liste : ce sont les `IsPrimary`, ils vivent dans la
bande d'icônes (§5).

**Ce que l'original délègue et ce qu'on a dû réécrire.** Files s'appuie sur
SevenZipSharp, `IWindowsShortcutService` et le shell ; côté Rust :

* raccourcis `.lnk` : `IShellLink` + `IPersistFile` (`file_ops::create_shortcut`
  / `resolve_shortcut`) — c'est exactement ce que fait le service original ;
* archives : `tar.exe` (bsdtar, livré avec Windows depuis 1803) crée et lit les
  `.zip` ; `7z.exe` prend le relais pour `.7z` / `.rar` quand il est installé ;
* « Envoyer vers » : on énumère le dossier `SendTo` de l'utilisateur, comme
  l'Explorateur — l'original, lui, laisse le shell remplir le sous-menu.

Non portés (la fonctionnalité elle-même n'existe pas encore) : « Modifier les
étiquettes » (`AddNewFileTagsToMenu`, dépend du magasin d'étiquettes),
« Définir comme » / Rotation (images), BitLocker, Corbeille.

### 7. « Afficher plus d'options » : `ItemOverflow` et son « Chargement … »

Dans l'original, l'entrée `ItemOverflow` naît avec `Text = Loading`,
`IsEnabled = false`, puis `AddShellMenuItemsAsync` interroge `IContextMenu` et
remplace ce contenu. On reproduit le même cycle :

1. `show_item_context_menu` crée l'entrée avec un unique enfant « Chargement … »
   désactivé, puis **poste** `WM_APP_SHELL_MENU` ;
2. le message étant traité *après* le repaint, la ligne « Chargement … » est
   réellement affichée avant que la requête (lente : elle réveille toutes les
   extensions shell installées) ne démarre ;
3. `ShellMenu::open` fait `QueryContextMenu` dans un HMENU jetable puis énumère
   ses entrées (`GetMenuItemInfoW` / `GetMenuStringW`, ampersands et colonne
   d'accélérateur retirés) ; l'objet `IContextMenu` est **conservé** dans
   `MainWindow.shell_overflow`, car les verbes ne sont valides que sur celui qui
   a produit les identifiants ;
4. les entrées deviennent des `MenuCommand::ShellCommand(i)`, et un clic fait
   `InvokeCommand`.

Les entrées shell porteuses d'un sous-menu sont écartées : notre flyout ne
descend que d'un niveau, et l'overflow occupe déjà ce niveau.


### 8. Les icônes du shell dans le sous-menu overflow

Une extension shell fournit l'icône de sa ligne via `MIIM_BITMAP` : un `HBITMAP`
32 bpp posé sur le `MENUITEMINFOW`. Il appartient au menu et meurt avec lui, donc
`ShellMenu::open` en recopie les pixels tout de suite (`GetDIBits`, hauteur
négative pour obtenir un DIB top-down), les prémultiplie — Direct2D l'exige — et
les range dans `FlyoutItem.bitmap`, l'équivalent du `BitmapIcon` de
`ContextMenuFlyoutItemViewModel`. Les entrées natives de Windows (Couper, Copier,
Supprimer, Propriétés…) n'en portent pas : le shell les dessine lui-même, elles
restent donc sans icône, exactement comme dans l'original.

### 9. L'animation d'ouverture

Files **n'override aucune transition** de flyout (aucun `PopupThemeTransition`,
`EntranceThemeTransition` ni `Transitions` sur ses flyouts dans tout le dépôt) :
l'animation vient du `PopupThemeTransition` par défaut de WinUI, dont les valeurs
vivent dans les ressources de thème du SDK, pas dans le code de Files.

On anime **la fenêtre popup elle-même** (et pas son contenu) : elle grandit de 34 %
à 100 % de sa hauteur en 170 ms, avec l'assouplissement décélérant de Fluent
(`1-(1-t)³`, la forme du cubic-bezier 0.1,0.9,0.2,1). Comme c'est le popup qui
grandit, DWM anime avec lui l'acrylique, l'ombre et les coins arrondis ; le
contenu, lui, est peint à sa place définitive, si bien que les lignes se
révèlent. On part de 34 % et non de zéro : un popup de 1 px n'a ni coin ni ombre,
et l'apparition « clignoterait ». Le sous-menu a sa propre horloge
(`Flyout::set_submenu` la remet à zéro), donc il se déplie pareillement.
Le timer `MENU_ANIM_TIMER` (16 ms) réarmé par `sync_flyout` cadence le tout.

**Le piège qui a coûté l'animation.** `QueryContextMenu` réveille toutes les
extensions shell de la machine : ~400 ms. Postée en message, la requête était
traitée AVANT le premier `WM_PAINT` (les messages postés passent avant les
peintures), donc le thread UI gelait avant même que le menu n'apparaisse, et le
dépliage était intégralement sauté. La requête vit désormais sur un thread STA
dédié (`shell_menu::ShellWorker`) — ce que fait l'original avec sa `Task`. Ce
thread **garde** l'`IContextMenu` : les verbes ne sont valides que sur l'objet qui
a produit les identifiants, donc l'invocation repasse par lui.


### 10. Survol décalé : le recadrage écran n'était pas réinjecté dans l'état

`place_popup` recadre chaque panneau sur la zone de travail du moniteur — c'est
justement ce qui permet au menu de déborder de la fenêtre. Mais le test de survol
(`Flyout::hit`) travaillait toujours sur la position *théorique* (`flyout.x/y` et
l'ancre calculée du sous-menu) : dès qu'un panneau était recadré, la ligne allumée
n'était plus celle sous la souris.

`sync_flyout` réinjecte donc la position RÉELLE (reconvertie en DIP client par
`screen_to_client_dip`) dans `flyout.x/y` et dans le nouveau `flyout.sub_pos`, dont
`sub_panel_rect` se sert en priorité. `place_popup` étant idempotent, la position
se stabilise dès la première image.


### 11. Le bouton « Disposition » : un `Flyout`, pas un `MenuFlyout`

`Toolbar.xaml` (`LayoutOptionsButton`) n'ouvre pas un menu mais un vrai panneau —
un `<Flyout>` dont le contenu est un `StackPanel Spacing="12"` :

1. le titre « Disposition » ;
2. cinq `RadioButton` de **76×72** (`Local.RadioToggleButtonStyle`), icône
   ThemedIcon 28 au-dessus du libellé en 12 pt ;
3. un filet **plein cadre** (`Margin="-20,0"` — c'est lui qui trahit le padding
   de 20 du `Flyout`) ;
4. « Taille », un `Slider` (`SnapsTo="Ticks"`, `TickPlacement="BottomRight"`)
   dont le `Maximum` dépend de la disposition (5 / 5 / 4 / 12 / 5), puis la
   rangée d'icônes de taille : seuls certains crans sont marqués, et l'icône du
   cran courant est `IsFilled` ;
5. un second filet ;
6. deux `ToggleSwitch` : « Éléments masqués » et « Extensions de fichiers ».

Le panneau vit dans `controls/layout_flyout.rs` et réutilise le popup acrylique
des menus (`FlyoutWindow`), d'où le paramètre `layout: Option<&LayoutPanel>` de
`present`/`paint` : le popup peint soit un menu, soit ce panneau.

Deux pièges :

* **Le clic ne doit PAS fermer le panneau.** Les menus se referment au
  `WM_LBUTTONUP` parce que `ReleaseCapture` y déclenche `WM_CAPTURECHANGED`. Ici
  il faut rester ouvert : le `WM_LBUTTONUP` traité pendant `menu_capture` appelle
  `on_flyout_click` **sans** rendre la capture, et seul un clic hors de
  `panel_rect()` ferme.
* **Le libellé des `ToggleSwitch` vient des commandes**
  (`ToggleShowHiddenItemsAction.Label` → `Strings.HiddenItems`), et le texte
  On/Off vient de **WinUI**, pas de Files : il n'est donc dans aucun .resw porté
  (`drive_localization::toggle_on_off`).


### 12. `GetIconSize` n'est pas la taille d'affichage

`LayoutSizeKindHelper.GetIconSize` donne la taille de la **vignette demandée au
shell** (96 / 128 / 256 en grille) — pas celle à laquelle elle est peinte. En
grille, `GridViewBrowserTemplate` pose l'image dans une boîte carrée de
`ItemWidthGridView` avec `Margin="12"` et `Stretch="Uniform"` : l'icône fait donc
`largeur − 24`, et la tuile mesure `largeur × (largeur + ligne du nom)`. Les
avoir confondues donnait des icônes de 128 dans des tuiles de 100.

La carte (`CardsBrowserTemplate`), elle, est un vrai cadre (CardBackground,
CardStroke, rayon 8) fait d'une **boîte d'aperçu** (`CardsViewIconBox*`, fond
`SystemFillColorNeutralBackground`) et d'une **boîte de détails**
(`CardsViewDetailsBox*`, nom en BodyStrong + type), accolées horizontalement pour
la petite taille et empilées au-dessus (`CardsViewOrientation`).


### 13. Les volets sont redimensionnables

Deux poignées, deux mécaniques différentes dans l'original :

* **`SidebarResizer`** (`SidebarView.xaml`) : un `Border` de 4 DIP au bord droit
  du volet (`Margin="0,60,0,40"`, `Canvas.ZIndex="100"`), curseur
  `SizeWestEast`. `ManipulationStarted` mémorise la largeur (`preManipulation
  SidebarWidth`), `ManipulationDelta` fait `largeur + translation.X` et le passe
  à `UpdateDisplayModeForPaneWidth` : **sous `COMPACT_MAX_WIDTH` (200), le volet
  bascule en mode Compact** ; au-dessus il reste Étendu et la largeur va dans
  `AppearanceSettingsService.SidebarWidth` (défaut 255, bornée 180–500 par
  `Constants.UI.Minimum/MaximumSidebarWidth`). Un double-clic bascule
  Étendu ⇄ Compact. Faute d'un mode Compact ici, on referme le volet — le même
  geste que le bouton hamburger.

* **`InfoPaneSizer`** (`MainPage.xaml`) : un `GridSplitter` de 2 DIP à gauche du
  volet. La largeur vit dans `InfoPaneSettingsService.VerticalSizePx` (défaut
  250, jamais moins de 100), et c'est le `MinWidth="208"` de `ContentColumn` qui
  borne sa croissance — le volet ne peut pas dévorer la liste.

Côté Rust : `Hot::SidebarResizer` / `Hot::InfoPaneResizer` passent **en tête** de
`hit_test` (le `ZIndex=100` de l'original), `MainWindow::pane_drag` garde
`(poignée, x de prise, largeur de prise)`, et `WM_SETCURSOR` pose `IDC_SIZEWE`.
Le volet d'informations étant à droite, il grandit quand la souris va à
**gauche** : `largeur − Δx`.


### 14. Préférences d'affichage : elles sont PAR DOSSIER

`LayoutPreferencesManager` garde disposition, tri et sens du tri **par dossier**
(base `LayoutPreferencesDatabase`, doublée d'un flux ADS sur le dossier lui-même)
— sauf si `SyncFolderPreferencesAcrossDirectories` est vrai, auquel cas tout le
monde partage les valeurs globales du `LayoutSettingsService`.

`layout_prefs.rs` en reproduit la sémantique, pas la plomberie : la « base » est
une table `chemin → préférences` dans les réglages. `Tab::set_location` les
relit avant de lister (comme `BaseShellPage.OnNavigatedTo`), et tout changement
de disposition ou de tri les réécrit. Le FRN (qui permet à l'original de suivre
un dossier renommé) n'a pas d'équivalent utile ici.


### 15. Colonnes : un `BladeView`, pas une liste

`ColumnsLayoutPage` n'est pas une disposition de la liste : c'est un `BladeView`
de lames de **200 DIP** (`BladeItem`, bord droit d'1 px), chacune un
`ColumnLayoutPage` listant son dossier. Sélectionner un dossier ouvre une lame à
sa droite et **referme toutes celles qui suivaient** (`DismissOtherBlades`), et
le chemin courant suit la sélection. La pile défile horizontalement.

D'où `Tab::columns` (`Vec<ColumnPane>`) et `select_column_row`. Attention :
l'ouverture d'une lame met à jour `location` et l'historique **sans** reconstruire
la pile — sinon on perdrait les colonnes de gauche à chaque clic.


### 16. La `ScrollBar` a deux états, et le premier est presque invisible

Files ne surcharge pas le style de la `ScrollBar` (`App.xaml` ne touche qu'à
`IsScrollInertiaEnabled`) : c'est la barre WinUI standard, et elle a deux visages.

* **Au repos** — un simple *panning indicator* : le pouce seul, **2 DIP** de
  large, arrondi, sans piste ni flèches, et il n'apparaît que **pendant** le
  défilement avant de s'effacer. Relevé au pixel sur l'app originale : 4 px de
  large à 175 %, #9C9EA4 sur un fond #262B38 — soit du blanc à 54,5 %, exactement
  `ControlStrongFillColorDefault` (#8BFFFFFF). Notre rendu donne #9C9C9C sur
  #262626 : même valeur.
* **Déplié** — la barre complète de 12 DIP (`ScrollBarSize`) dès que le pointeur
  entre dans la gouttière : piste, pouce de 6 DIP et les deux `RepeatButton`
  fléchés, un clic dans la piste valant une page.

Piège de mesure : `PrintWindow` ne capture pas la barre de l'app originale, et
l'auto-masquage de Windows l'efface en ~1,5 s — il faut molette **et** capture
dans le même processus, sinon on photographie une barre déjà partie.

Au passage : l'en-tête de `DetailsLayoutPage` est **hors** du `ScrollViewer`.
Les lignes doivent donc être écrêtées sous lui (`PushAxisAlignedClip`), sans quoi
elles lui passent par-dessus dès qu'on défile.


### 17. L'arborescence Rust recopie celle de `Files.App`

Pour comparer le port à l'original fichier par fichier, `crates/drive-app/src/`
suit désormais le même découpage que `Files-main/src/Files.App/` : `data/` ↔
`Data/`, `services/` ↔ `Services/`, `helpers/` ↔ `Helpers/`, `utils/` ↔
`Utils/`, `view_models/` ↔ `ViewModels/`, `user_controls/` ↔ `UserControls/`,
`views/layouts/` ↔ `Views/Layouts/` (Details / Grid / Columns, l'ancienne
`file_area.rs` découpée comme les pages originales — `GridLayoutPage` héberge
bien les TROIS dispositions Liste/Cartes/Grille, comme en C#). La table
complète, y compris ce qui n'a pas d'homologue (`ui.rs`, `graphics.rs` : le
rôle du runtime XAML) et ce qui reste à extraire (`Actions/`, `Dialogs/`), est
dans `docs/ARCHITECTURE.md`.


### 18. La multi-sélection

`Tab.selected` est passé de `Option<usize>` à un `BTreeSet<usize>` — le
`SelectedItems` de `ShellViewModel` — accompagné d'une `selection_anchor` (le
point de départ d'un Maj+clic, l'élément à focus du ListView original).

Les gestes de l'Explorateur : clic nu = sélection réduite à l'élément (et pose
l'ancre), Ctrl+clic = bascule, Maj+clic = bloc contigu depuis l'ancre, et le
clic DROIT ne réduit la sélection que si l'élément visé n'en fait pas partie.
Copier/Couper/Supprimer/Partager opèrent sur TOUTE la sélection ; Renommer sur
le premier. La barre d'état affiche « N éléments sélectionnés » via la
ressource `SelectedItems` — un pluriel ICU (`{0, plural, one {…} other {…}}`),
évalué par un mini-interpréteur (`icu_plural`).

Cela a débloqué `actions/content/selection.rs` : SelectAll (Ctrl+A),
InvertSelection, ClearSelection — et le menu « Options de sélection » de la
barre, dont les trois entrées étaient grisées. `ToggleSelectAction`
(Ctrl+Espace) attend encore l'élément à focus clavier.


### 19. Le trio dossiers/fichiers du tri

`SortFilesFirst` + `SortDirectoriesAlongsideFiles` forment un trio RADIO
(« dossiers d'abord / fichiers d'abord / mélangés »), porté par trois actions
(`SortFoldersFirstAction` etc. → `actions/display/sort_files_first_action.rs`)
et rangé PAR DOSSIER dans `LayoutPreferencesItem` (nos `LayoutPreferences`,
`serde(default)` pour les préférences déjà enregistrées). Le comparateur
`sort_entries` prend un `SortGrouping`, et l'inversion du sens ne s'applique
toujours QUE dans chaque groupe.

Le flyout Trier de la barre (Toolbar.xaml) se termine par un séparateur puis
ces trois entrées à coche — ajoutées au nôtre, indices 5–7 du `SortMenu`.

Deux pièges rencontrés :
* `..sep.clone()` propageait `separator: true` aux entrées du trio, qui se
  peignaient en filets de 3 DIP — un menu à la bonne LISTE mais à la mauvaise
  HAUTEUR signale ce genre d'héritage de champ.
* La disposition Colonnes garde son listing par lame : `set_sort`/`sort_by`
  doivent appeler `rebuild_columns()`, sinon le nouveau tri n'apparaît que dans
  les autres dispositions.

Au passage : le repli d'icône d'un FICHIER sans vignette (Cartes/Grille/
Colonnes/Liste) affichait le dossier jaune ; c'est maintenant le glyphe de
fichier (`file_glyph`), comme en Détails.


### 20. Clavier de liste, Entrée, Ctrl+Espace, souris 4/5

Le clavier du ListView (pas des actions C#) : Haut/Bas déplacent le FOCUS
(`Tab.focused`, la tête de sélection) — nu = sélection simple, Maj = étend
depuis l'ancre, Ctrl = déplace le focus seul. Entrée = `OpenItemAction`
(HotKey Enter, via le registre), Ctrl+Espace = `ToggleSelectAction`, et
`WM_XBUTTONUP` route les boutons souris 4/5 vers `NavigateBack`/`Forward`
(leurs `HotKey` `Mouse4`/`Mouse5` — le registre ne connaît que le clavier).

Piège de TEST (pas de code) : les flèches sont des touches ÉTENDUES. Injectées
par `keybd_event` SANS `KEYEVENTF_EXTENDEDKEY`, le pilote synthétise un faux
Maj-relâché (compatibilité pavé numérique) et ni `GetKeyState` ni
`GetAsyncKeyState` ne voient Maj pendant le message — Maj+Bas simulé passait
pour un Bas nu. Le drapeau 0x1 (+ scancode 0x50) répare l'injection ; le code,
lui, était correct pour un vrai clavier.


### 21. Le groupement (`GroupAction`)

`GroupOption` (None/Nom/Date de modification/Type/Taille) est porté avec les
clés de groupe EXACTES de `GroupingHelper.GetItemGroupKeySelector` :

* **Nom** — la première lettre, en capitale ;
* **Date de modification** — l'échelle de `ToTimeSpanLabel` (Aujourd'hui /
  Hier / Plus tôt cette semaine / La semaine dernière / Plus tôt ce mois-ci /
  Le mois dernier / Plus tôt cette année / L'année dernière / « Année N »),
  portée dans `services/date_time_formatter::time_span_label` avec le rang de
  tri (`SortIndexOverride`) ;
* **Type** — dossier → son type, fichier → son extension en minuscules ;
* **Taille** — les bandes de `sizeGroups` (16 Ko / 1 Mo / 128 Mo / 1 Go / 5 Go),
  les dossiers dans leur groupe à part.

Mécanique : un tri STABLE par clé de groupe PAR-DESSUS le tri des éléments
(`GroupedCollection`) — chaque groupe garde son ordre interne. Le layout
Détails insère une ligne d'en-tête de 36 DIP avant chaque frontière
(`Layout::group_rows` : libellé BodyStrong + effectif). Persistance par
dossier : `DirectoryGroupOption`/`DirectoryGroupDirection` dans
`LayoutPreferences`. Le flyout Grouper (Toolbar.xaml) : les options radio, un
séparateur, Croissant/Décroissant (grisés sans groupement) — et les sept
actions `GroupBy*`/`GroupAscending`/`GroupDescending` au registre.

Pas encore : les en-têtes dans Grille/Cartes/Liste (le groupement s'applique à
l'ordre, mais sans ligne d'en-tête), et `GroupByDateUnit` (Mois/Année).


### 22. Sidebar : sections Réseau et Étiquettes, chevrons

Le port de `SidebarView`/`SidebarViewModel` avance : les quatre sections
(Épinglé, Lecteurs, Réseau, Étiquettes) sont conditionnées par
`ShowXxxSection` et dépliables par leur chevron (`IsXxxSectionExpanded`,
persisté — défauts de l'original : Épinglé/Lecteurs dépliés, Réseau/Étiquettes
repliés).

* **Réseau** : les lecteurs `DRIVE_REMOTE`, désormais énumérés (un lecteur
  réseau déconnecté sans espace mesurable reste listé) et rangés là plutôt que
  dans Lecteurs.
* **Étiquettes** : `FileTagsSettingsService.DefaultFileTags` verbatim (Home
  #0072BD, Work #D95319, Photos #EDB120, Important #77AC30), l'icône teintée
  de la couleur du tag. La navigation `tag:` viendra avec la recherche —
  l'entrée est visible mais inerte.

Reste de la task : l'ARBORESCENCE des lecteurs (chevron par élément qui déplie
ses dossiers enfants, le vrai `SidebarItem` hiérarchique) et le défilement de
la sidebar (les éléments au-delà de la hauteur sont tronqués).

L'arborescence est faite : `SidebarEntry::Folder(chemin, profondeur)`, un
ensemble `sidebar_expanded` (chemins dépliés) et un cache `sidebar_children`
rempli AU DÉPLIAGE (`load_directory` filtré aux dossiers — il honore déjà
« Éléments masqués ») pour que le layout, recalculé à chaque image, ne touche
jamais le disque. Le chevron à droite de chaque lecteur/dossier déplie sans
naviguer ; le reste de la ligne navigue. Parcours en profondeur avec pile
explicite, indentation 16 DIP par niveau.

Piège d'outillage : `sed`/`perl` mangent les séquences `\u{…}` des littéraux
Rust (en GNU sed, `\u` MAJUSCULE le caractère suivant) — les glyphes Réseau et
Étiquettes se sont retrouvés en texte littéral `{E968}` à l'écran. Écrire les
échappements via `\N{U+005C}` en perl, ou vérifier le fichier après coup.

Correction utilisateur (capture de l'original à l'appui) : le chevron est
DEVANT la ligne — à gauche de l'icône — pas en bout de ligne : sections
(chevron 4–24, icône 28, libellé en couleur SECONDAIRE) et éléments d'arbre
(chevron 24–44 + 16/niveau, icône 48). La zone de clic du chevron suit. Et un
lecteur réseau MAPPÉ reste dans Lecteurs (l'original y affiche aussi l'UNC —
« html (\serveur) (N:) », à porter via WNetGetConnectionW) ; la capture
montre aussi la section « Lecteurs cloud » (iCloud, Nextcloud…) encore absente
(`CloudDrivesDetector` : détection par registre).

Le ScrollViewer de la sidebar : `sy` court sur TOUT le contenu (décalé de
`sidebar_scroll`) et seules les lignes entièrement visibles sont poussées dans
le layout — l'étendue se mesure au bout de la course, la molette au-dessus de
la sidebar défile SON contenu (borné par l'étendue), Paramètres reste épinglé
en pied. Pas encore de barre de défilement visuelle ni de lignes partielles
écrêtées (elles disparaissent d'un cran entier).


### 23. Lecteurs cloud et UNC des lecteurs réseau

`utils/cloud.rs` porte `CloudDrivesDetector` : le détecteur OneDrive
(`HKCU\...\OneDrive\Accounts\*` → « OneDrive - Nom ») et le détecteur
générique — les extensions d'espace de noms de l'Explorateur épinglées
(`Desktop\NameSpace\{clsid}` + `CLSID\{clsid}` avec
`System.IsPinnedToNameSpaceTree=1`, dossier dans
`Instance\InitPropertyBag\TargetFolderPath`), le type identifié par préfixe
d'identifiant (iCloudDrive, iCloudPhotos, ownCloud, ProtonDrive, SyncCom) puis
par `ApplicationName` (Nextcloud, kDrive), et la table de noms verbatim (MEGA
avec le nom du dossier, Nextcloud avec l'identifiant complet…). Vérifié sur la
machine : les DEUX comptes Nextcloud, OneDrive, iCloud Drive et iCloud Photos
— identiques à la capture de l'original. La section « Lecteurs cloud »
(`SidebarCloudDrives`) s'insère entre Lecteurs et Réseau, dépliée par défaut,
arborescence comprise. Restent : Sharepoint/Yandex/pCloud/Nutstore/Seadrive/
Autodesk, et les icônes de marque (`DefaultIcon` des CLSID).

L'UNC : `WNetGetConnectionW` (feature `Win32_NetworkManagement_WNet`) dans
`DriveItem::display_name` — « html (\serveur\partage) (N:) », y compris pour
un lecteur déconnecté.


### 24. En-têtes de groupes en Grille/Cartes, et la ScrollBar de la sidebar

Le `GroupStyle` du GridView : un en-tête pleine largeur avant chaque groupe et
l'habillage repart à la ligne — porté dans les branches Grille et Cartes du
layout (compteur de colonne explicite, le `i % per_row` ne survit pas aux
ruptures de groupe). La Liste (habillage vertical) reste sans en-têtes.

La sidebar a sa propre ScrollBar (demande utilisateur) : le même composant que
le contenu, avec sa plomberie parallèle (réveil au défilement, dépliage au
survol de la gouttière, glissement du pouce, page au clic dans la piste) et le
même timer d'effacement partagé. Vérifiée au pixel : l'indicateur de 2 DIP à
#A9A9A9 dans la gouttière droite de la sidebar.


### 26. Le socle des Dialogs : `DynamicDialog` en Direct2D

`user_controls/dialogs/` porte le `ContentDialog` de `DynamicDialog.xaml` :
un voile plein-fenêtre (`SmokeFillColorDefault`), une carte centrée de 448 DIP
(rayon 8 `OverlayCornerRadius`, titre 20 SemiBold, sous-titre), et la bande de
boutons en retrait (`SolidBackgroundFillColorBase`, filet, primaire ACCENT à
parts égales avec Fermer). Modal de bout en bout : le hit-test ne laisse
passer que les deux boutons, Entrée = primaire (`DefaultButton="Primary"`),
Échap = fermer, tout le reste est bloqué.

Premier consommateur : `DeleteItemAction` ouvre le `DeleteItemsDialog` quand
`DeleteConfirmationPolicy == Always` (titre `DeleteItemsDialogTitle`,
sous-titre `DeleteItemsDialogSubtitle` — pluriels ICU). Au passage,
`icu_plural` préservait mal le texte AUTOUR du bloc `{0, plural, …}` :
« Supprimer {0, plural, one {l'élément} …} » perdait son « Supprimer » ;
l'évaluateur remplace maintenant le bloc entier dans la chaîne.

Prochaines boîtes sur ce socle : CreateArchiveDialog, DecompressArchiveDialog
(les actions Compresser/Extraire les attendent), AddItemDialog.


### 27. CreateArchiveDialog : le premier dialogue à contenu riche

Le socle s'est étendu : `DialogState` porte un champ texte (« Nom », édité par
le `EditState` habituel sous `EDIT_DIALOG` — le clavier du dialogue délègue à
`on_edit_key` tout ce qui n'est pas Entrée/Échap) et un choix (« Format », des
pilules zip/7z, `Hot::DialogChoice`). La géométrie est devenue dynamique
(`dialog_rects(dialog, …)` calculée dans le Layout, sous-titre/champ/choix
optionnels).

`CompressIntoArchiveAction` ouvre désormais le vrai `CreateArchiveDialog`
(titre `CreateArchive`, nom prérempli par `archive_name`, Créer/Annuler) au
lieu de compresser en zip directement ; l'OK lit le nom du champ et le format
choisi. Niveau de compression, découpage et mot de passe de l'original restent
à porter (le backend `compress` ne les connaît pas encore).

Vérifié à l'écran depuis : champ « Nom » prérempli sélectionné, pilules
zip/7z qui basculent, Créer/Annuler. Deux corrections au passage : un clic
DANS le dialogue (une pilule) passait par le « clic hors éditeur = valider le
renommage » du WM_LBUTTONUP et détruisait le texte du champ — l'édition
`EDIT_DIALOG` vit maintenant tant que le dialogue est ouvert, comme le TextBox
du ContentDialog ; et `draw_edit_text` écrête désormais texte/sélection/caret
à la boîte avec un défilement horizontal qui garde le caret visible (un long
chemin débordait de la carte).


### 28. DecompressArchiveDialog et la case à cocher du socle

`DialogState` porte une case à cocher optionnelle (`checkbox_label`/
`checkbox`, `Hot::DialogCheckbox`, boîte 20 DIP rayon 4, coche `E73E` sur fond
accent) — la CheckBox pleine largeur des XAML.

`DecompressArchiveAction` (Ctrl+E) ouvre le vrai `DecompressArchiveDialog` :
titre `ExtractArchive`, champ `ExtractToPath` prérempli
`parent\nom-sans-extension` (`DefaultDestinationFolderPath`), case
`DecompressArchiveDialogOpenDestinationWhenComplete.Content` qui navigue vers
la destination après extraction. Vérifié à l'écran de bout en bout (extraction
réelle + navigation). Restent de l'original : le bouton Parcourir
(IFileOpenDialog), les suggestions `PreviousArchiveExtractionLocations`, mot
de passe et encodage (le backend `decompress` ne les connaît pas).

Le `DeleteItemsDialog` gagne la case « Supprimer définitivement »
(`chkPermanentlyDelete` de FilesystemOperationDialog.xaml) : cochée, la
suppression retire `FOF_ALLOWUNDO` (`delete_items(paths, permanently)`).
Vérifié à l'écran.

Pièges d'outillage appris en vérifiant : `lclick2`/`rc` postent des px client
BRUTS que l'app divise par le scale (175 % ici) — viser en px, pas en DIP ;
une fenêtre recouverte par d'autres ne reçoit aucun vrai clic (la passer
`HWND_TOPMOST` sans activation et vérifier `WindowFromPoint` avant chaque
`SendInput` pour ne jamais cliquer dans les fenêtres de l'utilisateur) ; le
premier vrai clic sur une fenêtre inactive est absorbé par l'activation —
cliquer deux fois ; et les items de menu ne réagissent qu'aux VRAIS clics (la
fenêtre principale tient la capture, un `WM_LBUTTONUP` posté au popup passif
est ignoré).


### 29. AddItemDialog : la liste et ses deux dialogues de nommage

Le socle porte une liste cliquable (`DialogState.list` de `DialogListItem` —
icône 24, titre, sous-titre gris ; rangées de 50 DIP comme le template de
`AddItemDialog.xaml` ; `Hot::DialogItem`) et sait n'afficher QU'UN bouton
pleine largeur quand `primary_text` est vide (l'AddItemDialog n'a
qu'« Annuler »).

`AddItemAction` (Ctrl+Maj+I) ouvre la liste `AddDialog.Title` : Dossier
(E838), Fichier (E8A5), Raccourci (E71B) — glyphes et sous-titres du
ViewModel C# verbatim. `ListView_ItemClick` : le clic enchaîne sur
`GetFor_CreateItemDialog` (« Créer un nouveau {dossier|fichier} », champ
prérempli « Nouveau dossier » sélectionné — #17845) ou sur le
`CreateShortcutDialog` (chemin cible → `.lnk` si la cible existe, `.url`
sinon, nom « {cible} - Raccourci » à la `CreateShortcutAsync`, collisions par
suffixe « (n) » — `unique_path`). Vérifié à l'écran : dossier créé et
`Windows - Raccourci.lnk` pointant sur `C:\Windows` (WorkingDirectory `C:\`).
Les entrées ShellNew du registre (`.accdb`, `.docx`…) restent à porter.

Deux corrections au passage : le filtre des caractères interdits
(`\/:*?"<>|`) du WM_CHAR ne s'applique plus qu'aux éditeurs de NOMS (rename,
CreateArchive, CreateItem) — il bloquait « C: » dans les champs de CHEMIN et
dans l'omnibar, alors que `IsValidForFilename` ne vaut que pour les noms ; et
`create_shortcut` normalise les « / » en « \ » (`IPersistFile::Save` échoue
silencieusement sur des séparateurs Unix, héritables d'un argv).


### 30. Sidebar : les vraies icônes (fin des glyphes approximatifs)

La sidebar reproduit maintenant les sources d'icônes exactes de
`SidebarViewModel.CreateSection` :

* **Sections** — Épinglé/Accueil : `Star.png`/`Home.png` (déjà en place) ;
  Lecteurs cloud : `CloudDrive.png` ; Étiquettes : `FileTags.png`
  (`assets/fluent-icons/`, chargés par `load_images`) ; Lecteurs et Réseau :
  `imageres.dll` par INDEX (`Constants.ImageRes.ThisPC` = 109, `Network` =
  25) via `utils::shell::imageres_icon` (SHDefExtractIconW → DrawIconEx dans
  un DIB 32bpp top-down) et le cache `IconCache::ensure_imageres`/
  `get_imageres` (clé « imageres:{index} »).
* **Items** — lecteurs (réseau compris), dossiers de l'arborescence et
  dossiers de synchro cloud : l'icône SHELL du chemin (`IconCache`), ce qui
  donne d'office les icônes de MARQUE des fournisseurs cloud (Nextcloud,
  OneDrive, iCloud…) — l'écart « icônes DefaultIcon » se règle tout seul ;
  étiquettes : la géométrie `FilledTag` TEINTÉE de la couleur du tag
  (`FileTagItem.IconElement`), plus le glyphe E8EC.
* **Menus « Nouveau »** — Dossier/Fichier passent aux géométries ThemedIcon
  `Folder`/`File` (la géométrie `File` rejoint `themed-icons.txt`).

La passe `ensure` de main_window précharge les icônes des entrées sidebar
visibles (culling déjà en place). Vérifié à l'écran : épinglés avec vraies
icônes système (Corbeille comprise), lecteurs avec icônes de marque (Google
Drive), UNC réseau, cloud avec marques, étiquettes 4 couleurs.

Non-écarts confirmés : les rangées des pages Paramètres et l'AddItemDialog
utilisent des `FontIcon` (glyphes) DANS L'ORIGINAL AUSSI — nos `Icon::Glyph`
y sont fidèles tels quels.


### 31. Vue Cartes : la sélection fidèle (capture utilisateur à l'appui)

Écarts relevés en comparant une carte sélectionnée à l'original :

* **Bordure de sélection** : ACCENT de 2 DIP
  (`GridViewItemSelectedBorderThickness` du conteneur « Regular » de
  GridLayoutPage.xaml), tracée vers l'intérieur (`stroke_rounded_w`) — le fond
  de la carte reste `CardBackground` (pas de voile pressé) ; le survol passe
  au fond Tertiary. La GRILLE, elle, n'a PAS de bordure : sa sélection est le
  fond `GridViewItemBackgroundSelected` seul (son conteneur met
  `SelectedBorderBrush` à Transparent).
* **`SelectionCheckbox`** : boîte de 20 DIP, marge 6, coin haut-gauche de la
  zone d'aperçu — cochée (fond accent, coche E73E) quand l'item est
  sélectionné, vide au survol (`UpdateCheckboxVisibility`). Et elle AGIT :
  `Hot::FileCheckbox` bascule la sélection de l'item au clic
  (`ItemSelected_Checked`/`Unchecked`), le clic droit y ouvre le menu de
  l'élément, et le hit-test ne s'active qu'en Grille/Cartes
  (`Layout.grid_checkboxes`).
* **Ligne de taille** (`FileSize`, caption secondaire) en bas de la boîte de
  détails, comme le StackPanel du bas du template.
* **Icônes réelles enfin chargées** : la passe `ensure` préchargeait du
  18 DIP quel que soit le mode alors que Cartes dessine en 64–96 et que
  Détails dessine à `icon_dip.max(ICON_ROW_DIP)` — la clé du cache doit être
  CELLE du dessin, mode par mode. Au passage, les échecs d'extraction sont
  maintenant loggés (`icon fetch failed`), et les chemins d'argv sont
  normalisés en « \ » dès l'entrée (les « / » cassent IShellItem, donc toutes
  les icônes du dossier ouvert).

Vérifié à l'écran : carte sélectionnée identique à la capture de référence
(bordure 2 DIP, case cochée, vraie icône zip, « 774,00 octet(s) »), et la
désélection par la case fonctionne.


### 32. Balayage des bordures de sélection : plus aucun double trait

La capture utilisateur de la page Apparence a révélé le motif fautif :
simuler une bordure épaisse par DEUX `stroke_rounded` de 1 px concentriques
(rects décalés d'un `inflate`) rend une DOUBLE ligne. Balayage complet des
`stroke_rounded` accent du code :

* **Tuiles « Couleurs du fond »** (appearance_page) : une seule bordure
  accent de 2 DIP (`stroke_rounded_w`) au lieu des deux traits.
* **Barre d'adresse en mode édition** (navigation_toolbar) : idem — l'anneau
  de focus est UNE bordure accent de 2 DIP.
* **Champ d'édition** (rename inline et champs de dialogue, edit_box) : le
  focus d'un TextBox WinUI est un bord discret (`card_stroke`) + un SOULIGNÉ
  accent de 2 DIP en bas — plus d'anneau accent de 1 px.
* Les autres `stroke_rounded` restants sont des hairlines de cartes/flyouts
  (card_stroke, divider) — légitimes ; le point du slider du panneau
  Disposition est un remplissage, pas une bordure.

Vérifié à l'écran : tuile « Par défaut » (bordure simple 2 DIP) et barre
d'adresse en mode chemin (anneau simple).


### 33. Les ComboBox déroulent NOTRE flyout, plus le menu Win32

Les listes déroulantes des pages Paramètres passaient par `TrackPopupMenuEx`
— le vieux menu système, à mille lieues du ComboBox WinUI. Les trois
`dropdown*` reposent maintenant sur `dropdown_flyout` :

* le menu est un `Flyout` ordinaire (`FlyoutKind::Combo`) rendu dans le popup
  acrylique existant — coins arrondis, ombre DWM, animation d'ouverture ;
* l'item sélectionné est rendu à la `ComboBoxItem` : fond subtil + pilule
  accent (`FlyoutItem.pill`) sur le bord gauche, à la place de la coche des
  `ToggleMenuFlyoutItem` ;
* le contrat SYNCHRONE des ~14 points d'appel est préservé par une boucle de
  messages imbriquée (le fonctionnement interne de `TrackPopupMenuEx`) : la
  wndproc réentrante traite survols, clics et Échap, `on_flyout_click` relève
  le choix dans `combo_pick`, et `dropdown` rend l'index en base 1 quand le
  flyout se ferme.

Vérifié à l'écran sur le combo « Thème » : items corrects, pilule accent sur
« Sombre », sélection au clic relevée (`pick=Some(2)`), rejet au clic hors
menu. Piège de test au passage : le flyout tient par `SetCapture`, refusé aux
fenêtres non-foreground — les clics POSTÉS le ferment aussitôt ; vérifier aux
vrais clics, fenêtre au premier plan.

Complément : le menu s'ancre maintenant SOUS le bouton du ComboBox, bord
gauche aligné, au moins aussi large que lui (`combo_anchor`, posé par
`on_settings_row_id` et `on_appearance_row` ; `place_popup` recadre déjà sur
le moniteur s'il sortait de l'écran) — plus jamais au point de clic. Et la
colonne de navigation des Paramètres colle à `SettingsPage.xaml` : titre
`Margin="16,12,0,4"` (Subtitle), premier item à 44 DIP — elle était décalée
de 20 DIP vers le bas.


### 34. Le ColorPicker du bouton « Couleurs du fond »

Le bouton damier du header ouvrait… rien. Il ouvre maintenant un port du
`ColorPicker` du CommunityToolkit (le Flyout d'AppearancePage.xaml,
`IsAlphaEnabled="True"`), dans le popup acrylique existant
(`user_controls/color_picker.rs`, `Flyout.picker` — le principe du panneau
Disposition) :

* bande d'aperçu sur damier, deux onglets (spectre / curseurs par canal),
  carré teinte×saturation rendu à PLEINE valeur (comme le `ColorSpectrum` —
  la 3e dimension appartient au curseur de gauche), curseur de valeur
  (couleur→noir), curseur d'alpha (damier), knobs ronds ;
* onglet canaux : quatre rails R/V/B/A en dégradé + valeur hex ;
* la couleur s'applique EN DIRECT (`AppThemeBackgroundColor` TwoWay,
  `#AARRGGBB`) à chaque geste, sans reconstruire les popups ;
* interaction Slider véritable : les zones continues s'arment au bouton-BAS
  (WM_LBUTTONDOWN) et suivent la souris jusqu'au relâchement — les armer au
  clic (bouton-haut) laissait le glissement actif entre deux clics, et les
  mouvements suivants polluaient l'état (teinte ramenée à 0) ; le
  relâchement d'un glissement ne vaut pas clic (finir hors du panneau ne le
  ferme pas).

Vérifié à l'écran : ouverture depuis le damier, spectre fidèle, sélection
verte conservée après réglage de la valeur, application live persistée
(réglage remis à `#00000000` après les tests).


### 35. Comparaison côte à côte avec l'original compilé (pages Paramètres)

Lancer l'app originale (WindowsApps) à côté du port et confronter les mêmes
zones a rendu trois écarts, tous corrigés :

* **Titre de page** (« Général », « Apparence »…) : il s'ALIGNE sur le titre
  « Paramètres » — style Subtitle + `Padding="0,0,0,12"` dans le
  ScrollViewer paddé 12 (GeneralPage.xaml) ; il était 28 DIP trop bas et les
  premières cartes avec lui (104 → 56 DIP).
* **ToggleSwitch** : l'original affiche l'état en toutes lettres à gauche de
  la piste (« Activé »/« Désactivé » — les chaînes plateforme de WinUI,
  absentes du resw de Files) ; `toggle_state_text` les fournit pour les
  cultures embarquées (fr/de/es/it/pt, anglais sinon), version grisée pour
  les interrupteurs désactivés.
* **`imageres_icon` extrayait la MAUVAISE icône** :
  `ExtractSelectedIconsFromDLL` passe `-index` à `SHDefExtractIcon` — les
  `Constants.ImageRes` sont des ID DE RESSOURCE (négatifs à l'appel), pas
  des ordinaux. Avec l'ordinal positif, « Lecteurs » et « Réseau » tiraient
  des icônes plausibles mais fausses ; corrigé (Ce PC -109 affiche bien
  l'écran bleu). Non-écart confirmé au passage : les items ÉPINGLÉS de
  l'original gardent l'icône shell de leur chemin — seules les SECTIONS
  passent par imageres (LoadSidebarIconResources).

Vérifié à l'écran contre l'original : titres alignés, « Désactivé » sur les
toggles, icône Ce PC correcte, épinglés inchangés.


### 36. Les épinglés RÉELS de l'Accès rapide

La comparaison côte à côte suivante (sidebar) a montré que l'original liste
les épinglés du VRAI Accès rapide de l'utilisateur (Google Drive, dossiers
personnels…), là où notre modèle synthétisait les six dossiers connus. Port
de `WindowsQuickAccessService.GetPinnedFoldersAsync`
(`services/windows_quick_access_service.rs`) : énumération du dossier shell
« Emplacements fréquents » (`::{3936e9e4-d92c-4eee-a85a-bc16d5ea0819}` — le
dossier Accès rapide lui-même y ajouterait les fichiers récents), filtre
`System.Home.IsPinned` (clé résolue par `PSGetPropertyKeyFromName`, lue par
`IShellItem2::GetBool`) + `SFGAO_FOLDER`, chemins `SIGDN_FILESYSPATH`, noms
`SIGDN_NORMALDISPLAY`. Les dossiers connus ne restent qu'en repli si le
dossier virtuel est indisponible ; la Corbeille reste ajoutée comme avant.

Vérifié à l'écran : notre sidebar liste exactement les épinglés de
l'original (Google Drive, CanadaWorkspaceVPS, dwhelper, Finances, VOYAGE…)
avec leurs icônes shell. Reste à confronter au prochain cycle : la vue
Détails (en-têtes, densité) et la barre d'état.


### 37. Redimensionnement de la barre latérale : mode Compact, et non « fermeture »

Relecture de `SidebarView.xaml(.cs)` après un doute de l'utilisateur sur le
redimensionnement des blocs : notre poignée « fermait » le volet sous une
largeur minimale inventée (180) et clampait à 180–500. L'original ne ferme
jamais le volet au glissement — il le fait BASCULER :

- `COMPACT_MAX_WIDTH = 200` : tiré en deçà, `DisplayMode` passe à `Compact` ;
  au-delà, il revient à `Expanded`. Le glissement reste continu (clamp
  `MINIMAL_..` → `MAX_WIDTH = 500`, notre `SIDEBAR_MAX_WIDTH`).
- `SidebarCompactOpenPaneLength = 56` : en Compact, le volet devient un rail
  de 56 DIP — icônes de SECTIONS centrées, sans libellé ni chevron, enfants
  masqués (`ui::SIDEBAR_COMPACT_WIDTH`, `sidebar_width()` renvoie 56).
- `SidebarResizer_DoubleTapped` bascule Étendu ⇄ Compact (`WM_LBUTTONDBLCLK`),
  comme le bouton hamburger. L'état vit dans `settings.sidebar_compact`.
- Aucune animation sur cette bascule : les transitions de 350 ms (spline
  0.1,0.9,0.2,1.0) du XAML n'existent que pour le mode `Minimal` (le volet
  qui GLISSE par-dessus le contenu sur fenêtre étroite) — pas encore porté.

Piège de hit-test : la `ScrollBar` de la barre latérale longe le même bord que
la poignée ; son pouce recouvrait les 4 DIP du `SidebarResizer` et absorbait
le double-clic. Le XAML donne `Canvas.ZIndex="100"` aux poignées : dans
`Layout::hit_test`, les deux resizers passent donc AVANT les scrollbars.


### 38. Alignement gauche de la région de contenu (ContentPresenter 2,0,8,0)

Trois flèches rouges de l'utilisateur pointaient le même écart : barre de
commandes, carte de contenu et barre d'état ne partageaient pas le bord gauche
de l'original. Vérité XAML :

- `MainPage.xaml` : la barre d'outils (`uc:Toolbar Margin="0,0,0,4"`) et la
  vue sont hébergées par `SidebarView.InnerContent`, dont le `ContentPresenter`
  porte `Margin="2,0,8,0"`. Tout le contenu commence donc à `sidebar + 2` DIP
  et finit à `largeur - 8` — cmdbar comprise (elle n'a AUCUN retrait latéral
  propre).
- `StatusBar.xaml` : `<Grid Padding="8,0,0,0">` — le texte à 8 DIP du bord
  gauche de cette même région.

Dans `ui.rs`, `content_left = sidebar_w + 2.0` sert à la fois à la cmdbar
(boutons à `content_left + 12`) et à la carte de fichiers ; `status_bar.rs`
écrit à `bar.left + 8.0`.

Vérifié à l'écran contre l'original (même dossier, vue Cartes) : « + Nouveau »,
le bord gauche de la carte et « N éléments » tombent sur la même verticale.

Restes identifiés pour le prochain cycle : la cmdbar de l'original expose plus
de boutons (partage, outils) ; le flyout des enfants d'une section en mode
Compact ; le mode `Minimal` et ses animations de 350 ms.


### 39. UN SEUL arrière-plan : barre latérale + gouttière + dessous des cartes

Retour utilisateur répété (« il s'agit du même arrière-plan », « la sidebar ne
partage toujours pas le même arrière-plan ») avec trois flèches rouges sur la
cmdbar, la carte de contenu et la barre d'état. Mesures au pixel de l'original
(dossier Téléchargements), rangée sidebar → contenu :

| zone | original | nous (avant) |
|---|---|---|
| barre latérale | `#1A212A` | `#2C2C2C` |
| gouttière | `#191F29` (≈ sidebar) | `#1A1A1A` (**bande sombre**) |
| zone fichiers | `#262C36` (carte + claire) | `#262626` (plus foncé) |

Deux causes, deux correctifs :

1. **`card_stroke` était blanc à 7 %** (`rgba(255,255,255,0.07)`) alors que le
   vrai `CardStrokeColorDefault` sombre est `#19000000` = NOIR à 10 %. Ce
   liseré clair encadrait chaque bloc. Corrigé en `rgba(0,0,0,0.098)` (le
   commentaire connaissait déjà la valeur, seul `tab_border` l'appliquait).

2. **La barre latérale ne teintait que sa propre colonne** ; la gouttière et le
   dessous des cartes montraient le fond de fenêtre nu (sombre) → couture. Dans
   l'original, `App.Theme.Sidebar.BackgroundBrush` (= `LayerOnMicaBaseAlt`)
   forme une base translucide UNIQUE sous toute la zone (barre latérale +
   gouttière + dessous des cartes), et les cartes se posent PLUS CLAIRES
   par-dessus (`CardBackgroundFillColor…`). Porté en posant `layer_fill` sur
   tout `Rect::new(0, TAB+TOOLBAR, width, height)` en tête de `draw()`, puis en
   supprimant le remplissage propre de la sidebar (sinon 45 % + 45 %).

Résultat mesuré : sidebar `#2C2C2C`, gouttière `#242424` (≈ sidebar, résidu
d'ombre), contenu `#303030` (carte plus claire) — la relation de l'original est
retrouvée, la bande sombre a disparu.


### 40. Barre d'outils : jeu de commandes EXACT (ToolbarSections + Toolbar.xaml)

Sur demande de reproduire dimensions / style / icônes / fonctions à l'identique.
Source : `ToolbarSections.DefaultItemsByContext[AlwaysVisible]` (bloc gauche) et
`Toolbar.xaml` `BaseCommandBar` (bloc droit).

- **Gauche (primaires)** : `Nouveau ▾` · **AppBarSeparator** · Couper · Copier ·
  Coller · Renommer · **Partager** · Supprimer · **Propriétés**. Nous
  manquaient le séparateur, Partager (`ShareItem`) et Propriétés
  (`OpenProperties` — l'icône « clé » vue au bout dans l'original). Icônes
  vectorielles `Share`/`Properties`/`NewItem` (déjà présentes dans
  `themed-icons.txt`). Clics câblés via `run_menu_command` (Partager sur la
  sélection ; Propriétés sur la sélection ou, à défaut, le dossier courant).
- **Droite (`BaseCommandBar`)** : Filtre · Options de sélection ▾ · Trier ▾ ·
  Disposition ▾ · Volet d'aperçu (bascule accent). Le bouton « Grouper » séparé
  a été RETIRÉ : dans l'original le groupement est un `MenuFlyoutSubItem` DANS
  le flyout Trier. Chevron ajouté au bouton Disposition (c'est un flyout).

Reste pour un prochain cycle : peupler le flyout Trier avec le sous-menu
Grouper ; l'icône exacte `SelectMode` (nous réutilisons `SelectAll`).


### 41. ThemedIcon multi-couches : accent SEULEMENT là où l'original le rend

Le système `ThemedIcon` de `Files.App.Controls` compose chaque icône de
plusieurs `ThemedIconLayer` (`LayerType` = Base/Alt = premier plan, Accent =
accent, AccentContrast = `TextOnAccentFillColorPrimary` = blanc). MAIS une
icône ne rend ses calques (donc l'accent) QUE si `IconColorType=None` ; sinon
le contrôle bascule sur le tracé unique `OutlineIconData` en UNE couleur
(monochrome).

Vérité mesurée sur la barre d'outils originale :
- **Bloc DROIT** (`BaseCommandBar`, `<ThemedIcon Style=…/>` sans IconColorType
  → calques) : Filtre (ligne du bas bleue), Options de sélection = SelectMode
  (badge coche bleu), Trier = Sorting (flèche haut bleue). Disposition reste
  monochrome (son style n'a pas de calque accent visible ici).
- **Bloc GAUCHE** (commandes primaires via `ApplyIcon`) : MONOCHROME blanc —
  Couper, Copier, Coller, Renommer, Partager, Supprimer, Propriétés.
- **Menus** (`MenuFlyoutItem`) : MONOCHROME.

Erreur corrigée : on avait appliqué le rendu multi-couches PARTOUT (gauche +
menus), ce qui produisait des pâtés blancs — la couche `AccentContrast` d'un
`New.Item` est un DISQUE plein (le fond du cercle) rendu en blanc, idem le
carré de `Shortcut`. En monochrome ces icônes utilisent leur tracé unique
`OutlineIconData` (cercle VIDE + « + »), pas les calques.

Implémentation : `themed_icons.rs` parse un format `@role[@opacity] path` (une
ligne sans `@` = tracé mono unique, rétrocompatible) ; `vector_icon_layered`
peint chaque calque avec fg / accent / blanc. Seules Filter/Sorting/SelectMode
portent des calques dans l'asset ; tout le reste reste en tracé mono. Les sites
de dessin : `vector_icon_layered` UNIQUEMENT pour le bloc droit ; `vector_icon`
(mono) pour le bloc gauche et les menus.

Balayage complet (agents) : 96 icônes de l'original ont un calque accent, mais
la plupart ne s'affichent avec accent nulle part (rendues monochrome selon le
contexte). Données de calques extraites verbatim pour les 21 icônes qu'on
utilise, mais réinjectées seulement là où l'original les rend en calques.

## §42 — Sidebar : défilement HORIZONTAL (`HorizontalScrollMode=Enabled`)

`SidebarView.xaml` (`MenuItemHostScrollViewer`) : `HorizontalScrollBar
Visibility="Visible"` + `HorizontalScrollMode="Enabled"`, `Disabled` dans l'état
visuel `Compact`. L'`ItemsRepeater` ne contraint pas la largeur des lignes :
elles gardent leur largeur NATURELLE et débordent, d'où la barre horizontale
quand un libellé long (lecteur réseau UNC, nom OneDrive/Nextcloud complet)
dépasse la largeur du volet.

Reproduction (Rust) :
- `sidebar_visual(entry, model, active_location) -> SidebarVisual` : helper
  extrait du gros `match` de `draw_sidebar` (glyphe, libellé, indent, section,
  sélection) — partagé par le calcul de mise en page et le dessin (plus de
  duplication).
- `sidebar_row_width` = `8 + indent + 20(icône) + 10 + approx_text_width(label,14)
  + 12`. `sidebar_hextent` = max sur TOUTES les lignes (pas seulement visibles,
  pour que la barre ne clignote pas au défilement vertical), borné à
  `sidebar_full`.
- `Scrollbar::new(..., horizontal=true)` (la struct gérait déjà l'axe) posée sur
  la gouttière du BAS. `state.sidebar_hscroll` + `sidebar_hscroll_drag`.
- Dessin : décalage `dx = -hscroll` (borné à `hextent - sidebar_full`) appliqué à
  l'icône, au libellé (largeur naturelle, non tronqué) et au chevron ; toute la
  bande du volet est clippée (`PushAxisAlignedClip`) pour qu'un libellé qui
  déborde ne bave pas sur la carte de contenu. Nul en Compact.
- Entrées : molette **Maj+molette** (`MK_SHIFT`) au-dessus du volet = axe
  horizontal ; glissement du pouce (`SidebarHScrollThumb`) ; clic de piste
  (`SidebarHScrollTrack`) = une page. Même loi d'effacement que la verticale.

## §43 — Mode Minimal de la barre latérale + animation 350 ms

`SidebarView` a 3 modes (`SidebarDisplayMode`) : Expanded / Compact / **Minimal**.
`MainPage` (`SidebarStates`, `MinWindowWidth="641"`) force **Minimal** quand la
fenêtre fait < 641 DIP : le volet devient un **overlay flottant** de 300 DIP
(`SidebarOpenPaneLength`) par-dessus le contenu, masqué (`TranslateX=-300`) et
révélé par le bouton hamburger (`IsPaneOpen` → MinimalCollapsed ⇄ MinimalExpanded),
avec une couche de light-dismiss.

Animation : seul le `TranslateX` du volet est animé — **350 ms**
(`KeyTime="0:0:0.35"`), spline **`0.1,0.9 0.2,1.0`** ; les changements de largeur
sont discrets. Implémenté par `cubic_bezier_ease(0.1,0.9,0.2,1.0)` + timer 16 ms
(`SIDEBAR_ANIM_TIMER`), interpolant `sidebar_pane_tx`.

Reproduction (Rust) :
- `SidebarMode` + `sidebar_mode(window_width)` ; layout : `sx0` (origine X du
  volet = `TranslateX` animé en Minimal, 0 ancré sinon), `sidebar_w=0` en Minimal
  (le contenu prend toute la largeur), `sidebar_overlay`/`sidebar_light_dismiss`.
- Toute la géométrie du volet (items, scrollbars, resizer, clip) est relative à
  `sx0`. Resizer `Collapsed` en Minimal.
- Le volet flottant est peint avec `flyout_background` (acrylique OPAQUE,
  `AcrylicBackgroundFillColorDefaultBrush`) + ombre + bordure.
- **Ordre de dessin** : la barre latérale est dessinée APRÈS le contenu et la
  barre de commandes/état — sinon, en Minimal, les cartes recouvriraient le volet.
- Hamburger (mode Minimal) bascule `IsPaneOpen` + lance le glissement ;
  `SidebarLightDismiss` (région à droite du volet) le referme ; `WM_SIZE`
  réconcilie le mode (franchir 641 est discret).

## §44 — Popups de menu : DPI de dessin dérivé (anti-compression multi-écran)

Chaque menu contextuel est une **fenêtre popup séparée** (`FilesFlyoutPopup`).
`sync_flyout` la dimensionne à `w_px = width_dip * (dpi_fenêtre_principale / 96)` ;
son contenu est peint à l'échelle `dpi_popup / 96`. Si le popup a mis en cache le
DPI d'un moniteur différent de celui où il s'affiche (multi-écran hétérogène),
les deux DPI divergent et le texte apparaît **horizontalement comprimé** (ou
étiré). Correctif dans `FlyoutWindow::present` : on **dérive** le DPI de dessin
du rapport fenêtre/contenu voulu — `self.dpi = 96 * w_px / width_dip` — de sorte
que le contenu remplit *toujours* exactement la largeur de la fenêtre, quel que
soit le moniteur. Sur un seul écran, cela redonne le DPI courant (aucune
régression).

## §45 — Flyout « Trier » : Trier par + Grouper par imbriqués (profondeur 3)

`Toolbar.xaml` n'a **qu'un** bouton « Trier » (`ArrangementOptions`, icône
`Sorting`) — **pas** de bouton « Grouper » distinct. Son `MenuFlyout` porte :
`Trier par` (sous-menu), `Grouper par` (sous-menu), un séparateur, puis le trio
radio `Trier les dossiers en premier` / `…les fichiers en premier` /
`…ensemble` (`SortFoldersFirst`…). Dans « Grouper par », les entrées de date
(`Date de modification`) se déploient sur un **3ᵉ niveau** Année/Mois/Jour
(`GroupByDateUnit`). Le bouton `SelectMode` (options de sélection) est un
ThemedIcon à calques : sa pastille accent + coche blanche (`@accent` +
`@accentcontrast`) est rendue par `vector_icon_layered`.

Reproduction (Rust) — le moteur de flyout supportait 2 niveaux ; on l'a étendu
à **3** de façon additive :
- `Flyout` : champs `subsubmenu`/`subsubmenu_opened`/`subsub_pos`/`hot_sub2`,
  méthodes `subsub_items`/`subsub_panel_rect`/`subsub_item_rect`/`set_subsubmenu`,
  variante `FlyoutHit::SubSubItem`. `set_submenu` referme le 3ᵉ niveau.
- `sync_flyout` : **3** fenêtres popup (`menu_popups[0..3]`) — menu, sous-menu,
  3ᵉ niveau — chacune positionnée/animée comme les précédentes.
- Survol : un `SubItem` qui a lui-même des enfants déploie le 3ᵉ niveau (même
  logique `MenuFlyoutSubItem` qu'au niveau 1).
- Le bouton `Grouper` distinct (`Hot::CmdGroup`, `FlyoutKind::GroupMenu`) est
  supprimé : le groupement vit dans le flyout Trier.

## §46 — Intégration Git dans la barre d'état (StatusBar.xaml colonnes 1 & 2)

Port de `Utils/Git/GitHelpers.cs` + `Services/Git/LibGit2Service.cs` +
`Data/Models/DirectoryPropertiesViewModel.cs` (la partie git de
`StatusBarViewModel`). L'original s'appuie sur LibGit2Sharp ; on utilise le
crate **`git2`** (liaisons Rust de libgit2, feature `vendored-libgit2`).
`build.rs` lie `advapi32` (Crypt*/jetons de sécurité que libgit2 référence).

- **Service** `utils/git.rs` : `discover` (remonte l'arbo jusqu'au `.git`),
  `head_info` (branche courante + `graph_ahead_behind` vs l'amont suivi ;
  `None` en HEAD détaché → masque toute l'UI git), `branches` (locales +
  distantes, triées par date de tête décroissante, plafond 30/groupe, tête en
  premier), `checkout`/`create_branch`/`delete_branch`, et les opérations
  réseau `fetch`/`pull`/`push`/`sync`. Auth : credential helper (HTTPS stocké)
  + agent SSH + identité par défaut (l'OAuth GitHub de l'original n'est pas
  reproduit). `sync` = pull PUIS push inconditionnel (comme le `ContinueWith`
  de `GitSyncAction`).
- **État** : `Tab.git_branch/git_ahead/git_behind`, recalculés dans
  `set_location`/`refresh` via `update_git_info` (`UpdateGitInfo`). Non nul ⇒
  dépôt ⇒ le widget s'affiche.
- **Rendu** (`status_bar.rs`) : à droite de la barre, un bouton d'actions réseau
  (icône `Git` + compteur « ahead / behind ») et un sélecteur de branche (icône
  `Git.Branch` + nom). Rects calculés dans `layout()`
  (`status_git_actions`/`status_git_branch`), hit-test → `Hot::StatusGitActions`
  / `Hot::StatusGitBranch`.
- **Flyouts** : `GitActions` (Pull/Push/Sync, icônes `Git.Pull/Push/Sync`) et
  `GitBranches` (liste des branches, coche sur la tête, clic = checkout ;
  séparateur ; « Créer une branche » → `DialogAction::GitCreateBranch`).
- **Exécution hors thread UI** : `run_git_op` lance l'opération bloquante sur un
  thread et poste `WM_APP_GIT_DONE` à la fin → `refresh` (comme
  `DoGitOperationAsync`).
- **Icônes ajoutées** à `themed-icons.txt` : `Git`, `Git.Branch`, `Git.Pull`,
  `Git.Push` (base+accent), `Git.Sync`, `Open` (accentcontrast+accent+base).
- **Reste à câbler** : le bouton « Ouvrir dans l'IDE » (dépend d'un réglage +
  détection d'IDE) et la suppression de branche par ligne (bouton corbeille de
  la `BranchesFlyout` custom — le flyout actuel est un menu). `delete_branch`
  est prêt côté service.

## §47 — Multi-volet (split) complété + sélection Colonnes + branches Git

Port de `Views/ShellPanesPage` (le split ≤ 2 volets) et des actions
`Actions/Navigation/*Pane*`. Le modèle (`TabGroup { panes, active_pane,
split_vertical }`) et le rendu des deux volets existaient déjà ; cette passe a
comblé les points d'entrée manquants :

- **Diviseur redimensionnable** (`GridSplitter`) : `TabGroup.split_ratio` (0.5
  par défaut, borné à ≥100 DIP/volet). Nouveau `Hot::PaneDivider`, testé AVANT
  le bloc `content` dans `hit_test` (il chevauche la frontière des volets),
  bande de saisie ±8 DIP. Glisser → recalcule le ratio depuis la position
  absolue ; double-clic → égalise (0.5) ; curseur `SizeWestEast`/`SizeNorthSouth`
  selon l'orientation. Réutilise le drag `pane_drag`.
- **`CloseActivePane`** (jusque-là injoignable) : entrée du menu contextuel de
  l'espace vide + du flyout tab-actions (offerte uniquement en multi-volet),
  et raccourci **Ctrl+Alt+W**.
- **Raccourcis** enregistrés dans le `CommandManager` (`actions/navigation`) :
  `SplitPaneVertically` **Alt+Maj+V**, `SplitPaneHorizontally` **Alt+Maj+H**,
  `CloseActivePane` **Ctrl+Alt+W**, `ToggleDualPane` **Ctrl+Maj+S**
  (`toggle_dual_pane` : ferme le 2e volet s'il existe, sinon en ouvre un).
- **Vue Colonnes** : `Tab.selected_entry()` renvoie désormais
  `column_selection()` (l'item de la lame la plus à droite) pour que le volet
  Détails le décrive ; `select_column_row` réutilise la lame de droite si elle
  montre déjà le dossier cliqué (`SetSelectedPathOrNavigate`), ce qui consomme
  `ColumnPane.path`.
- **Flyout branches Git** : compteur ahead/behind sur la tête (`↓ derrière
  ↑ devant`) et icône « cloud » sur les branches distantes — consomme
  `BranchItem.is_remote/ahead_by/behind_by`.

**Non porté (Adaptive)** : le mode de disposition `Adaptive` reste hors périmètre
(comme documenté dans `layout_action`). **Vestigial** (code mort/supplanté, non
des fonctionnalités manquantes) : `paste_into` (supplanté par
`paste_into_async`), les glyphes de légende `GLYPH_MIN/MAX/RESTORE/CLOSE` +
`close_hover` (la légende min/max/fermer est dessinée par le DWM),
`Action::glyph` (non consommé par les menus), `ICON_TILE_DIP`,
`SECTION_APPEARANCE`, `show_item_context_menu` (variante OS gardée à dessein),
`time_span_label`, `DriveItem.is_system`, `dropdown_ex`.

## §48 — Infobulles (`ToolTipService.ToolTip`)

Port des `ToolTipService.ToolTip` du chrome (`NavigationToolbar.xaml`,
`Toolbar.xaml`, `StatusBar.xaml`). L'original en pose deux sortes sur les
boutons à icône seule : `{x:Bind Commands.X.LabelWithHotKey}` — le libellé suivi
de « (raccourci) » — et `{helpers:ResourceString Name=X}` — un libellé simple.

- **Contenu** : `ui::tooltip_for(Hot) -> Option<String>` mappe chaque `Hot` du
  chrome à son texte. La forme `LabelWithHotKey` est reconstruite via
  `actions::by_name` (nouveau, l'indexeur `CommandManager[CommandCodes]`) :
  `tr(action.label())` + « (` + action.hotkey().text()` + ») », ce qui consomme
  `HotKey::text`/`hotkey_text_vk`. Couverts : Précédent/Suivant/Monter/Actualiser,
  Nouveau, Couper/Copier/Coller/Renommer/Partager/Supprimer/Propriétés, Filtre,
  Options de sélection, Trier, Disposition, volet d'informations, bascule de la
  barre latérale, centre d'état, branches Git.
- **Déclenchement** : sur changement de survol (`on_mouse_move`), un
  `SetTimer(TOOLTIP_TIMER, 750 ms)` est armé (`ToolTipService.InitialShowDelay`) ;
  passer d'un élément à l'autre réarme le délai. Le `WM_TIMER` ouvre l'infobulle
  de l'élément encore survolé. Elle se referme au `WM_MOUSELEAVE` et à tout
  `WM_LBUTTONDOWN`.
- **Rendu** : `Painter::draw_tooltip` dessine une pastille arrondie (rayon 4,
  fond `flyout_background`, bord `flyout_border`, texte `body`) posée sous le
  pointeur (`Placement=Mouse`, offset 22 DIP), bornée à la fenêtre (bascule
  au-dessus s'il n'y a pas la place en bas). Elle est peinte EN DERNIER, par-
  dessus tout le reste (le `ToolTip` est un `Popup` au sommet de l'arbre visuel).

## §49 — Barre d'état : taille de la sélection (`ItemSize`)

`StatusBar.xaml` affiche, après le compte d'éléments et le compte de sélection,
la **taille cumulée** de la sélection (`ItemSize`, séparateur `ItemSizeDivider`).
`draw_statusbar` (`user_controls/status_bar.rs`) somme désormais `entry.size` des
entrées sélectionnées et l'ajoute (`    |    9,16 Kio`). Fidèle au
`isSizeKnown` de `BaseLayoutPage.UpdateSelectionSize` : le bloc n'apparaît que si
TOUTES les tailles sont connues — un dossier (taille non calculée) le masque,
d'où la garde `sel.iter().all(|e| !e.is_dir)`.

## §50 — Parité du menu clic-droit : Disposition / Grouper par

Le menu contextuel de l'espace vide (`show_empty_space_menu`) ne proposait que
2 dispositions (Détails, Grille) et aucun « Grouper par ». Aligné sur
`ContentPageContextFlyoutFactory` :

- **Disposition** : les **5** modes (`ViewMode::ALL`), construits via
  `mode.icon()`/`mode.label_key()` ; nouvelles commandes `MenuCommand::LayoutList/
  Cards/Columns` déléguant à `actions/display/layout_action`. (Les icônes 28px
  `LayoutList28`… se dessinent à la taille du menu, faute de variante 16px.)
- **Grouper par** : sous-menu `None/Name/DateModified/Type/Size` + `Ascending/
  Descending` (désactivés si `GroupOption::None`), nouvelles commandes
  `MenuCommand::GroupBy*`/`GroupAscending/Descending` déléguant à
  `actions/display/group_action`. Le 3ᵉ niveau (Année/Mois/Jour de « Date de
  modification ») n'est pas répliqué ici — le menu générique ne gère que 2
  niveaux — ; la granularité fine reste joignable via le flyout « Trier » (§45),
  et « Date de modification » applique l'unité courante.

Le dispatch réutilise la voie éprouvée `command_at` → `run_menu_command` du menu
générique (identique au sous-menu « Trier par » déjà en place).

## §51 — Commandes `CopyPath` + centre d'état : compteur d'opérations

- **`CopyPath` / `CopyPathWithQuotes` / `CopyItemPathWithQuotes`**
  (`actions/file_system/mod.rs`, enregistrées dans `commands()`) : chemin du
  dossier courant (brut / entre guillemets) et chemin(s) de la sélection entre
  guillemets, un par ligne (`string.Join("\n", …)` de l'original) ; `CopyItemPath
  WithQuotes` porte le raccourci **Ctrl+Alt+C**. Elles apparaissent donc dans la
  palette de commandes et au clavier.
- **Centre d'état** (`draw_status_center_button`) : la pastille accent sans info
  cède la place à l'**InfoBadge** — le COMPTEUR d'opérations en cours
  (`InfoBadgeValue`, via `UiState.ops_count`). Le `MedianOperationProgressRing`
  déterminé n'est pas dessiné : `OpsMonitor` ne suit que `(id, libellé)`, sans
  pourcentage par opération — un anneau à valeur inventée trahirait la fidélité.
  À reprendre quand le worker rapportera une progression réelle.

## §52 — Fil d'Ariane : ellipse de débordement (`BreadcrumbBar.EllipsisButton`)

Le fil d'Ariane tronquait la QUEUE au débordement (le dossier courant
disparaissait). Aligné sur le `BreadcrumbBar` : au débordement, les segments de
TÊTE se replient derrière une ellipse (…) et les segments de queue restent
visibles.

- **Layout** (`compute`) : si la largeur totale dépasse, on garde toujours le
  dernier segment et on ajoute vers la gauche tant que ça tient ; `Layout.
  breadcrumb_ellipsis: Option<Rect>` (le bouton …) et `breadcrumb_start` (index du
  premier segment visible).
- **Rendu** (`navigation_toolbar.rs`) : l'ellipse (glyphe `E712`) + chevron avant
  les segments visibles ; ceux-ci s'indexent à `breadcrumb_start + k`.
- **Interaction** : `Hot::BreadcrumbEllipsis` ouvre le flyout `BreadcrumbOverflow`
  — les segments repliés (stockés dans `MainWindow.breadcrumb_overflow`),
  dispatché par index → navigation (même schéma que le flyout des branches Git).
  Le clic sur un segment visible mappe `breadcrumb_start + k` vers son chemin.

## §53 — Menu contextuel de la barre latérale enrichi

`show_sidebar_context_menu` ne proposait que Ouvrir (onglet/fenêtre), Épingler,
Propriétés. Aligné sur `SidebarViewModel.GetLocationItemMenuItems`, en réutilisant
le dispatch existant (`run_menu_command` + `flyout.path`) :

- **Ouvrir dans un nouveau volet** (sous-menu Vertical/Horizontal → split + navigation
  du nouveau volet), **Copier** (le dossier au presse-papiers, `CopyItemFromSidebar`),
  **Ouvrir le terminal** ici, **Afficher plus d'options** (menu shell via
  `request_shell_overflow`).

Non porté (dépend de sous-systèmes, cluster C) : Éjecter, Formater, Assistant
stockage (API shell de lecteur), Masquer cette section (drapeau par section),
Réorganiser (dialogue).

## Audit de parité (2026-07-15) — écarts restants vs l'original

Audit systématique original C# ↔ port, réconcilié (un « manque » dans une surface
donnée est souvent joignable par une autre). Constats :

- **Registre `commands()`** : 80/202 `CommandCodes`. MAIS la couverture réelle est
  bien supérieure — beaucoup de fonctions sont câblées directement dans les menus/
  flyouts sans passer par une struct `Action` formelle (ex. `GroupBy*`/`SortBy*`
  via le flyout « Trier », `DuplicateTab`/`CloseOtherTabs`/`ReopenClosedTab` via
  le menu contextuel d'onglet). Le déficit de `commands()` pèse surtout sur la
  **palette de commandes** (ce qui n'y est pas enregistré n'y apparaît pas).
- **Réglages** : ~100 % (seuls 3 réglages `IsAppEnvironmentDev` omis, à dessein).
- **Manques avérés, autonomes** (sans nouveau sous-système) : centre d'état sans
  anneau/compteur de progression (pastille seule) ; pas d'ellipse « voir plus » du
  fil d'Ariane ; parité du menu clic-droit (Grouper/Trier/Disposition n'y sont pas,
  bien que joignables via la barre de commandes) ; menu latéral réduit
  (Éjecter/Ouvrir dans un volet/Terminal/Masquer section/Réorganiser absents) ;
  commandes `CopyPath`/`CopyPathWithQuotes`.
- **Manques nécessitant un nouveau sous-système** (décision de périmètre requise) :
  volet Étagère (Shelf), manipulation d'images (Pivoter, Définir comme fond),
  opérations de lecteur (Formater/Éjecter/BitLocker/Assistant stockage), épingler
  au menu Démarrer, verbes d'installation (police/certificat/pilote), Annuler/
  Rétablir des opérations, superposition compacte/plein écran, aperçu rapide
  (QuickLook), bouton de mise à jour, modèles du menu « Nouveau ».

## §54 — Manipulation d'images : Pivoter + Définir comme fond

Port de `Actions/Content/ImageManipulation/` et `Actions/Content/Background/`,
plus le service `Services/Windows/WindowsWallpaperService.cs`.

- **Rotation** (`helpers/bitmap_helper.rs`, port de `BitmapHelper.RotateAsync`) :
  décode l'image (`BitmapDecoder`), la ré-encode en transcodage
  (`BitmapEncoder::CreateForTranscodingAsync`) vers un flux mémoire en appliquant
  la rotation à chaque trame, puis réécrit le flux par-dessus le fichier
  (préserve les métadonnées). Appels WinRT bloqués via `.join()`. Actions
  `RotateLeft` (270° horaire) / `RotateRight` (90°) dans
  `actions/content/image_manipulation.rs`. Vérifié : une image 240×120 devient
  120×240 après « Tourner à droite ».
- **Définir comme fond** (`actions/content/background.rs` +
  `services/windows_wallpaper_service.rs`) : Bureau (`IDesktopWallpaper.Set
  Wallpaper` par moniteur), Diaporama (`SetSlideshow` sur un `IShellItemArray`
  de PIDL), Verrouillage (`LockScreen.SetImageFileAsync`), Application (réglage
  `AppThemeBackgroundImageSource`). Nouvelles features windows-rs
  `Graphics_Imaging` + `System_UserProfile`.
- **Menu** : le menu clic-droit d'une image compatible (`is_wallpaper_
  compatible`) montre le sous-menu « Définir comme » (Bureau/Verrouillage/
  Diaporama/Application) et « Tourner à gauche/droite » ; enregistrées aussi dans
  `commands()` (palette + clavier). Les 6 sont surfacées via `MenuCommand`.

Note : le fond de bureau/verrouillage n'est PAS exécuté en test (il modifierait
le système de l'utilisateur) — porté fidèlement et vérifié à la compilation.

## §55 — Annuler / Rétablir (`StorageHistory`)

Port de `Actions/Global/Undo`+`Redo` et de la pile `StorageHistory` /
`StorageHistoryHelpers` (`utils/storage_history.rs`). Chaque opération enregistre
de quoi la réverter ; `try_undo` dépile l'annulation et applique l'inverse,
`try_redo` refait le trajet. Actions `Undo` (Ctrl+Z) / `Redo` (Ctrl+Y) dans
`actions/global`, enregistrées dans `commands()` (donc palette + clavier).

Couvre pour l'instant les opérations DÉTERMINISTES :
- **Renommer** : inverse = renommer vers le nom d'avant. Enregistré à la
  validation de l'éditeur (`end_rename`).
- **Créer** (dossier/fichier) : inverse = supprimer (corbeille) ; le rétablir
  recrée au chemin exact. `create_folder`/`create_text_file` renvoient désormais
  le `PathBuf` créé (nom unique via `unique_path`), et `create_item_at` recrée à
  un chemin précis. Tous les points de création passent par `MainWindow::
  create_item`, qui enregistre l'historique.

Vérifié : Ctrl+Shift+N crée « Nouveau dossier » → Ctrl+Z le supprime → Ctrl+Y le
recrée ; renommer alpha→omega → Ctrl+Z revient à alpha → Ctrl+Y refait omega.

**Prochain incrément** : déplacer/copier/supprimer. Ils passent par le
`IFileOperation` shell asynchrone (corbeille, renommage sur collision) ; leur
annulation demande de CAPTURER le résultat (chemins créés, emplacement corbeille)
via le sink d'événements — non encore remonté par la couche de stockage.

## §56 — Opérations de lecteur : Formater, Éjecter, Assistant Stockage

Port de `FormatDriveAction` + `Win32Helper.OpenFormatDriveDialog`,
`DriveHelpers.EjectDeviceAsync`, `OpenStorageSenseAction` /
`StorageSenseHelper`. Nouveau `utils/drive_helpers.rs` :

- **Formater** (`open_format_drive_dialog`) : ouvre la boîte de dialogue Windows
  `SHFormatDrive(hwnd, indexLettre, SHFMT_ID_DEFAULT, SHFMT_OPT_FULL)` (déclarée
  à la main, absente des métadonnées windows-rs). RIEN n'est effacé sans le clic
  de l'utilisateur DANS cette boîte. Action `FormatDrive` (exclut « C:\ »
  système et les racines non-lecteur).
- **Éjecter** (`eject_drive`) : le verbe shell « eject » via `ShellExecuteW`
  (pendant de `ContextMenu.InvokeVerb("eject")`). `MenuCommand::EjectDrive`.
- **Assistant Stockage** : action `OpenStorageSense` — `ShellExecuteW` sur l'URI
  `ms-settings:storagesense` (pendant de `Launcher.LaunchUriAsync`).

`FormatDrive` + `OpenStorageSense` sont enregistrées dans `commands()` (palette).
Les trois apparaissent sur une RACINE de lecteur (`is_drive_root`) dans le menu
latéral ET le menu d'une carte de lecteur ; « Formater » est masqué sur « C:\ ».
Vérifié à l'écran (menu C: : Éjecter + Assistant Stockage, pas de Formater).
Exécution NON testée (formater/éjecter un vrai lecteur serait destructif) —
portée fidèlement, vérifiée à la compilation.

## §57 — Volet Étagère (`ShelfPane`)

Port de `UserControls/Pane/ShelfPane.xaml`, `ViewModels/UserControls/
ShelfViewModel.cs`, `Data/Items/ShelfItem.cs` et `Actions/Show/
ToggleShelfPaneAction.cs`. L'Étagère est une zone de dépôt (largeur fixe 240) où
l'on glisse fichiers/dossiers pour y accéder entre onglets. **Réservée au build
Dev dans l'original** (`IsExecutable` gardé par `AppEnvironment.Dev`, TODO
« Remove when shelf feature is ready ») ; portée ici comme fonctionnalité à part
entière sur décision utilisateur (« Le porter quand même »).

- **Modèle** : `data/items.rs::ShelfItem { name, path, is_dir }`
  (`ShelfItem.cs` : `Name = storable.Name`, `Path = storable.Id` ; l'icône est
  chargée à la volée depuis l'`IconCache`, pas stockée).
- **Modèle de vue** : `view_models/shelf_view_model.rs::ShelfViewModel { items }`
  (`ObservableCollection<ShelfItem>`) avec `add_path`/`remove`/`clear`. Les
  `IFolderWatcher` par dossier parent (auto-retrait quand le fichier disparaît)
  sont remplacés par `prune_missing()`, balayé à chaque image — même effet, sans
  fil d'événements. Persistance = TODO côté original (`InitAsync`).
- **Réglage** : `AppSettings.show_shelf_pane` (`GeneralSettingsService.
  ShowShelfPane`), défaut `false`.
- **Bascule** : action `ToggleShelfPane` (glyphe `App.ThemedIcons.Shelf`,
  toujours exécutable ici), enregistrée dans `commands()`. Bouton `Hot::CmdShelf`
  dans le bloc droit de la barre de commandes, à côté de la bascule du volet
  d'aperçu (`ShelfPaneToggleButton`) — rempli d'accent quand l'Étagère est
  ouverte. Icône « Shelf » ajoutée à `assets/themed-icons.txt` (l'`OutlineIconData`
  de `Icons.Misc16.xaml`).
- **Disposition** : colonne à l'extrême droite (après le volet d'aperçu, comme
  `MainPage.xaml` ShelfPaneColumn) ; le contenu ET le volet d'aperçu se décalent
  vers la gauche. Trois rangées comme le XAML : en-tête « Étagère » + séparateur ;
  contenu = état vide (illustration + `EmptyShelfText`) OU liste `ShelfItemsList`
  (icône 16 + nom tronqué + × de retrait au survol) ; pied (si peuplée) =
  séparateur + lien « Effacer les éléments ». Rendu dans
  `user_controls/pane/shelf_pane.rs`.
- **Ajout par glissement** : le port n'a pas d'OLE drag-drop ; un glissement
  INTERNE (pendant de `DragItemsStarting` + `Shelf_Drop`) presse une ligne déjà
  sélectionnée, franchit un seuil de 4 DIP, et dépose les chemins saisis sur le
  volet au relâchement (`item_drag` dans `MainWindow`, cf. §58 — l'Étagère n'est
  qu'une des cibles de dépôt). Le cross-app OLE reste hors périmètre.
- **Interactions** : clic sur un article → `ViewInFolderAsync` (navigue vers le
  parent) ; × → `ShelfItem.Remove` ; « Effacer les éléments » → `ClearItemsCommand`.

Vérifié à l'écran (fenêtre de notre build uniquement, `PrintWindow` avec
vérification du PID) : état vide fidèle, bascule (bouton d'accent), glisser-déposer
d'un puis deux dossiers, et « Effacer les éléments » revenant à l'état vide.
L'illustration `EmptyShelf.48` (SVG dédié) est rendue par la géométrie ThemedIcon
« Shelf » agrandie — le port n'embarque pas encore ce SVG. Les actions
`Copy/Cut/DeleteItemFromShelf` (menu « Action groupée », `Collapsed` par défaut
dans l'original) ne sont pas encore portées.

## §58 — Glisser-déposer général des items (déplacer / copier)

Port de `Item_DragStarting` / `Item_DragOver` / `Item_Drop` (`BaseLayoutPage.cs`)
et de `FilesystemHelpers.PerformOperationTypeAsync`. Le port n'ayant pas d'OLE
drag-drop, un glissement INTERNE reproduit le déplacement/la copie d'items par
glissement ; le cross-app OLE (déposer depuis/vers un autre programme) reste hors
périmètre. Le `shelf_drag` de §57 est généralisé en `item_drag`.

- **Source** : presser une ligne DÉJÀ sélectionnée (`FileRow`) arme le glissement
  (chemins de la sélection, point de départ). Un seuil de 4 DIP le rend actif ;
  en deçà, le relâchement reste un clic ordinaire (la sélection n'est pas touchée).
- **Cibles de dépôt** (`drop_target`) : un dossier de la liste (`FileRow` d'un
  `is_dir`), une entrée de la barre latérale (dossier épinglé, lecteur, dossier
  déplié — pas la Corbeille ni les en-têtes), un segment du fil d'Ariane, un
  onglet (`Hot::Tab`), l'autre volet en mode double (`InactivePane`), ou
  l'Étagère (ajout, §57). Un fichier simple, une zone vide, Accueil/Réglages ne
  sont pas des cibles.
- **Opération** (pendant de la cascade `Item_DragOver`) : Ctrl force la copie,
  Maj force le déplacement ; sinon, même volume → déplacer, volume différent →
  copier (`volume_root` compare la lettre de lecteur, ou `\\serveur\partage` en
  UNC). On ne dépose pas un item sur lui-même, dans son propre sous-arbre, ni
  dans son dossier parent (déplacement sans effet). Le dépôt lance
  `paste_into_async` (IFileOperation, `FOF_ALLOWUNDO`) — donc annulable et suivi
  par le centre d'état, comme un collage.
- **Retour visuel** (`draw_item_drag`, `state.drag`) : la cible sous le pointeur
  est surlignée (bord + voile d'accent) et une légende suit le pointeur —
  « Déplacer vers X » / « Copier vers X » (`MoveToFolderCaptionText` /
  `CopyToFolderCaptionText`), avec une pastille de compte au-delà d'un item. La
  cible est résolue depuis la position VIVE du pointeur (et non `state.hot`, que
  `WM_MOUSELEAVE` peut effacer), comme le dépôt au relâchement.

Vérifié sur des COPIES en bac à sable (jamais de vrais fichiers) : déplacement
d'un fichier vers un sous-dossier (même lecteur, la légende « Déplacer vers X » et
la surbrillance d'accent s'affichent), et copie avec Ctrl (l'original reste, une
copie apparaît dans la cible). NON encore porté : le survol-pour-ouvrir
(`HoverToOpenTimespan`), le dépôt en RACCOURCI (Alt/Ctrl+Maj → `DataPackageOperation.Link`),
et le drag-drop OLE inter-applications.
