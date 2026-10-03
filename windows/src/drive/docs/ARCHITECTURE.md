# Architecture — correspondance avec `Files.App` (C#)

L'arborescence de `crates/kubuno-drive-desktop/src/` recopie celle de
`Files-main/src/Files.App/` pour permettre la comparaison fichier à fichier :
réviser un module Rust, c'est ouvrir son homologue C# à côté.

## Crates ↔ projets

| Rust | C# |
|---|---|
| `crates/kubuno-drive-desktop` | `src/Files.App` |
| `crates/kubuno-drive-desktop-app-storage` | `src/Files.App.Storage` |
| `crates/kubuno-drive-desktop-localization` | `src/Files.App/Strings` + `AppLocalizationService` |
| `crates/kubuno-drive-desktop-shared` | `src/Files.Shared` |

## Modules ↔ dossiers

| Rust (`src/…`) | C# (`Files.App/…`) | Contenu |
|---|---|---|
| `main.rs` | `App.xaml.cs` | Point d'entrée, culture, DPI, boucle de messages |
| `main_window.rs` | `MainWindow.xaml.cs` + `Views/MainPage.xaml.cs` | La fenêtre : wndproc, clics, clavier, commandes. Contient aussi ce que MainPage fait (positionnement des volets, capture des flyouts) |
| `data/items.rs` | `Data/Items/` | `DirEntryItem` (≈ `ListedItem`), lecteurs, accès rapide, `HomeModel` |
| `services/settings.rs` | `Services/Settings/` | Tous les réglages persistés (un champ par propriété des services C#) |
| `services/date_time_formatter.rs` | `Services/DateTimeFormatter/` | Dates « Il y a 4 jours » etc. |
| `helpers/layout_preferences.rs` | `Helpers/Layout/LayoutPreferencesManager.cs` | Préférences d'affichage PAR DOSSIER |
| `utils/storage.rs` | `Utils/Storage/` | Opérations fichiers : copier, raccourcis, archives, SendTo |
| `utils/folder_watcher.rs` | `ShellViewModel.WatchForDirectoryChanges` | Surveillance de dossiers |
| `utils/shell.rs` | `Utils/Shell/ContextMenu.cs` | `IContextMenu` (« Afficher plus d'options ») + worker STA |
| `utils/share.rs` | `Actions/Share/ShareItemAction.cs` | `IDataTransferManagerInterop` |
| `utils/thumbnails.rs` | `Utils/Storage/Helpers/FileThumbnailHelper.cs` | Cache d'icônes/vignettes shell |
| `view_models/shell_view_model.rs` | `ViewModels/ShellViewModel.cs` | `Tab` : listing, tri (`OrderFiles`), filtre, historique, lames Colonnes. Contient aussi `FolderLayoutModes` (C# : `Data/Enums`) et `LayoutSizeKindHelper` (C# : `Helpers/Layout`) |
| `user_controls/` | `UserControls/` | Voir le doc du module pour le détail contrôle par contrôle |
| `views/layouts/details_layout_page.rs` | `Views/Layouts/DetailsLayoutPage.xaml` | Lignes + en-têtes triables |
| `views/layouts/grid_layout_page.rs` | `Views/Layouts/GridLayoutPage.xaml` | Liste + Cartes + Grille (les 3 DataTemplate) |
| `views/layouts/columns_layout_page.rs` | `Views/Layouts/ColumnsLayoutPage.xaml` | Colonnes Miller (`BladeView`) |
| `views/layouts/mod.rs` | `Views/Layouts/BaseLayoutPage.cs` | Aiguillage + partagé (nom affiché, volet non focalisé) |
| `views/settings/*_page.rs` | `Views/Settings/*Page.xaml` | Une page par fichier, même découpage |
| `styles/theme.rs` | ressources thème (`Styles/`, tokens Fluent) | Couleurs claires/sombres |
| `styles/themed_icons.rs` | `Assets` + styles ThemedIcon | Géométries vectorielles des icônes |

## Sans homologue C#

| Rust | Rôle |
|---|---|
| `ui.rs` | Le « runtime XAML » : `Layout` (géométrie de toute la page, hit-testing) + `Painter` (primitives Direct2D). En C#, ce travail est fait par XAML lui-même |
| `graphics.rs` | Swap chain DirectComposition, Mica, formats DirectWrite, PNG |

## Pas encore porté (dossiers C# sans miroir)

- `Actions/` (172 fichiers) — chez nous, l'équivalent est le dispatch
  `run_menu_command`/`run_palette_command` dans `main_window.rs`. À extraire en
  `actions/` si l'on veut la comparaison 1:1.
- `Dialogs/` — les boîtes de dialogue (propriétés, conflits de copie…).
- `ViewModels/` autres que `ShellViewModel` (NavigationToolbarViewModel est
  fondu dans `main_window.rs`/`user_controls/navigation_toolbar.rs`).
- `Utils/Git/`, `Services/Git/` — l'intégration Git.
- `Views/Properties/` — la fenêtre Propriétés (nous n'ouvrons que la feuille
  de propriétés du shell).

## `actions/` ↔ `Actions/` (en cours)

Le trait [`Action`] (`actions/mod.rs`) porte `IAction` — `Label`, `Description`,
`Glyph`, `HotKey`, `IsExecutable`, `ExecuteAsync` — et `ToggleAction` porte
`IToggleAction.IsOn`. `actions::commands()` tient le rôle du `CommandManager` :
la palette y lit les descriptions, le clavier y résout les raccourcis
(`by_hotkey`), les menus les libellés. Un fichier Rust par fichier C#.

Migré : `display/layout_action.rs` (Ctrl+Maj+1…5), `display/sort_action.rs`, `file_system/` (Copy/Cut/Paste/Delete/Rename/CreateFolder/RefreshItems — Ctrl+C/X/V, Suppr, F2, Ctrl+Maj+N, Ctrl+R+F5), `navigation/` (NewTab Ctrl+T, NewWindow Ctrl+N, CloseSelectedTab Ctrl+W, Next/PreviousTab Ctrl+(Maj+)Tab), `global/edit_path` (Ctrl+L, Alt+D), `open/` (OpenSettings Ctrl+, — OpenTerminal Ctrl+` — OpenCommandPalette Ctrl+Maj+P — OpenProperties, EditInNotepad, OpenItemWithApplicationPicker), `content/archives` (Compress/Decompress, Ctrl+E), `content/run` (RunAsAdmin/RunAsAnotherUser), `content/share`, `sidebar/` (Pin/UnpinFolderToSidebar), plus OpenItem/OpenFileLocation/CopyItemPath (Ctrl+Maj+C)/CreateShortcut/PasteItemAsShortcut/CreateFolderWithSelection/FlattenFolder dans `file_system/`, et `navigation/` complété (NavigateBack Alt+Gauche+Retour, NavigateForward Alt+Droite, NavigateUp Alt+Haut, NavigateHome, OpenInNewTab/Window). Les actions restantes attendent leur FONCTIONNALITÉ : Selection/ (multi-sélection), GroupAction et SortFilesFirst/FoldersFirst (groupement et comparateur), Start/ (StartMenuService), Mouse4/Mouse5 (routage boutons souris),
`show/toggle_show_hidden_items_action.rs` (Ctrl+H),
`show/toggle_show_file_extensions_action.rs`, `show/toggle_info_pane_action.rs`
(Ctrl+Alt+I), `show/toggle_sidebar_action.rs` (Ctrl+B). Le reste des ~172
fichiers est suivi par la task #33 ; `Actions/Git/` est exclu (décision
utilisateur, juillet 2026).
