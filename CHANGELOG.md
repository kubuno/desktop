# Changelog

All notable changes to **kubuno-desktop** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this
project adheres to [Semantic Versioning](https://semver.org/). Entries are added under
`[Unreleased]` **as the change is made**; `_tools/release.sh` stamps them under the version
number at release time, and CI publishes that section as the GitHub Release notes.

## [Unreleased]

### Added

- **Four more type roles** (Badge 10 px, Caption 11 px, Subtitle 16 px, Display 24 px) join the shared web/desktop type scale, so
  small pills, group titles and large greetings use a named step instead of a free font size. They are available on
  `Label` and `LinkLabel` through `Role` in views (`Role="Badge"`, `"Caption"`, `"Subtitle"`, `"Display"`), after the
  existing values so nothing already written moves.

- **`{Res}` arguments and plurals** (vskubuno `docs/WEB-VIEWS.md` lot WV-6, the same rules as the web, which uses
  i18next). `{Res files, Count={Binding n}, Name={Binding user.name}, Sep=', '}`: every argument fills the string's
  `{{name}}` placeholders (`Count` fills `{{count}}`), and `Count` picks the plural form — `files_zero` for 0, else
  the culture's form (`files_one`, `files_few`, `files_many`, `files_other`… by the CLDR rules of the 13 product
  languages, checked against the browsers' `Intl.PluralRules`), else `files`. The text follows the bound values and
  the language live. A translation may hold forms its neutral file lacks (Russian `_few`, Arabic `_two`).
  `resources!` also generates `files(count)` for a key that only exists as plural forms. In `.kbres` files, a
  `String` may now be named like any web translation key (`header.settings`, `drive-shared`, `2fa_title`, spaces,
  colons) and every value comes back byte for byte; `Culture="en"` on a neutral file names the language of its
  strings. The views language server knows plural keys (completed once, hover lists the forms) and flags malformed
  arguments, and the web views compiler puts the arguments in the plan (`res.args`) and type-checks their bindings.

- **The views language server speaks web views too** (`kubuno-views-ls`, vskubuno `docs/WEB-VIEWS.md` lot WV-7). A
  `.kbview` / `.kbcontrol` of a web project (found from `kubuno.views.json`, a `package.json` using
  `@kubuno/views-compiler`, or a `.esproj`) is checked by the web compiler itself, against `@kubuno/ui`'s element
  registry and the project's own controls (re-read when they change), with the same messages as the web build,
  including the module isolation rule (an element or a code-behind import from another module is an error). Its
  TypeScript code-behind is read with oxc: completion of elements, attributes and values, `{Res}` keys from the
  project's locale bundles and `.kbres` files, hover, go to definition into the `.ts`, and the designer's handler
  requests (`kubuno/compatibleHandlers`, `createHandler`, `renameHandler`, `removeHandler`, F2), which insert
  methods and type imports without reformatting the file. The generated `.d.ts` under `.kubuno/views/` is rewritten
  as you type. Desktop views behave exactly as before.

### Changed

- **The header's panels follow the theme's `PanelBackground`.** The apps launcher and the account panel are tinted
  with a new theme colour, `PanelBackground` (`#E9EEF6` light, `#303134` dark, the web's `--color-panel-bg`), at
  80 % over their blur — in the dark theme the panel is now the web's dark ground instead of a near-black one. The
  colour is offered with the other theme colours in the views' colour editor.

- **Two title bar heights, buttons always centred.** Windows drawn by Kubuno now have a 32-pixel title bar, as in
  Windows 11 (dialogs, tool windows, secondary windows, documents opened inside a window, in-app dialogs such as
  confirmations), and main windows that show the header's menus (apps launcher, account, notifications…) have the
  web's 64-pixel header: the desktop shell, Chat and Documents (whose web editor also has a 64-pixel top bar above its
  ribbon). The new view property `TitleBarStyle` (`Standard` or `Tall`, also `Form::set_title_bar_style`) chooses
  it; left unset, a window showing the header's menus is `Tall` and any other `Standard`, and `TitleBarHeight` still
  sets an exact height. The minimise, maximise and close buttons, the icon, the title and the controls placed in the
  title bar are centred vertically in every height: Windows-style caption buttons now fill the bar's height instead of
  staying at its top (Chat's buttons were stuck to the top of its bar), and keep the snap layouts on the maximise
  button. The gallery has a new *titlebars* page showing both heights with both button styles. For testing, the
  `KUBUNO_UI_ZOOM` environment variable (formerly the gallery's own) now sets the initial zoom of every window.

### Added

- **More on-device storage** (lot ST-2). `KeyValueStore` (small values in a JSON file, optional expiry), `FileStore`
  (the app's files, a size-capped cache that drops the least recently used files, temporary files cleaned after a
  day) and `LocalDatabase` (a SQLite file of the app or of the signed-in account). Settings, key-value stores and
  files can be per account (`AccountScoped`), kept apart for each Kubuno account. Connection-string secrets now use the
  same names as the app's other secrets (`Kubuno/app.<id>/…`); the older names are still read and moved over.

- **Storage components for views and code** (vskubuno `docs/STORAGE-COMPONENTS.md`). A new cross-OS crate,
  `kubuno-app-storage` (desktop/common), keeps an app's data on the device: typed settings with user (roaming or
  this machine) and application (machine-wide, read-only) scopes, defaults, change notifications, changes made by
  another instance picked up within two seconds, and a versioned upgrade (renamed settings are moved, a newer file
  is never downgraded), stored as JSON files in the user's profile on Windows, Linux and macOS or, on request, in the
  Windows Registry; the app's secrets in the system's credential store; and a Windows Registry API with 32/64-bit
  views. Views get three components (`kubuno-app-storage-components`): `<Settings>` (bound with
  `{Binding Theme, Source=settings, Mode=TwoWay}`, saved at once), `<SecretStore>` (secrets are set from code; views
  only see whether one exists) and `<RegistryKey>` (Windows; read-only unless `Writable`). A `.kbsettings` file
  declares the settings and `kubuno::settings!("settings.kbsettings")` generates their typed class
  (`Settings::theme()`, `Settings::set_theme("Dark")`). Everything is scoped to the app; in a sandboxed profile
  (`KUBUNO_SANDBOX_DIR`) files, Registry keys and secret names stay inside the sandbox; values never reach the logs.

- **The title bar is the window's header, like the web's** (vskubuno `docs/SHELL-CONTROLS.md` §5). A window shows
  the web header's standard items before its caption buttons by setting `ShowSearch`, `ShowNotifications`,
  `ShowSettings`, `ShowHelp`, `ShowWaffle` and `ShowAccount` (off by default, for every kind of window), with the
  bell's `UnreadCount` and the events `SearchClicked`, `NotificationsClicked`, `SettingsClicked` and `HelpClicked`
  (code first: `Form::set_header_items`, `set_unread_count`, `search_clicked()`…). The items take the caption
  buttons' size in a usual title bar and the web's 36-pixel circles in the tall 64-pixel header, the header's
  neutral ground on an uncoloured band and the band's ink on a coloured one, and mirror for right-to-left windows.
  Any control can also go in the title bar's left, centre or right region (`TitleBar.Region`); a wide centre
  control no longer covers the right one. The space between the controls still moves the window, double-click
  still maximises and snap layouts stay on the maximise button. F6 (or Alt, in a window without a menu bar) moves
  the keyboard focus to the title bar's controls and back. Example: `kubuno-shell-controls --example form_header`.

- **Rounded window corners, by default and configurable.** Every Kubuno desktop window — main window, dialog, tool
  window, owned window, splash screen, MDI document, in-window dialog — now has rounded corners (8 pixels, like
  Windows 11), square while maximised, snapped or full screen. A view's `CornerRadius` (and `CornerPreference`)
  sets any radius, `0` for square corners; borderless windows stay square unless asked. On Windows 11 the default
  radius is drawn by the system (its shadow and border); any other radius, and every radius on Windows 10, is drawn
  by Kubuno with a soft shadow and a border following the curve, the window still resizing along the curve and the
  clicks in its rounded-off corners going to what is behind. Windows showing a Mica/Acrylic backdrop (Windows 11
  22H2 and later) or Windows' own title bar keep the system's nearest radius.
  For testing, `KUBUNO_CORNER_RADIUS=<pixels>` overrides every window's radius and `KUBUNO_CUSTOM_CORNERS=1`
  draws the corners as on Windows 10.

- **The header's menus are reusable controls.** A new shared crate, `kubuno-shell-controls`, holds the app launcher
  (`WaffleMenu`) and the account panel (`AccountMenu`) as user controls, the header buttons that open them
  (`WaffleButton`, `AccountButton`) and the web's right-hand header cluster (`HeaderActions`: bell with its counter,
  settings, help, waffle, avatar). Any Kubuno desktop app can place them in its views (they appear in the Visual
  Studio Toolbox of a project that depends on the crate, with sample data in the designer); their data come from
  the app through small `LauncherService` / `AccountService` traits. Names, properties and events are the same as
  on the web (vskubuno `docs/SHELL-CONTROLS.md`).
- **The launcher and the account panel open outside their window.** They open in a floating panel of their own
  (`kubuno::popup::Popup`: rounded, blurred, closing on a click outside or Escape), placed under their button,
  flipped above it when there is no room below and kept inside the screen's work area — so a small app window no
  longer clips them.
- **The account panel matches the web's**: the camera button on the photo (it opens the web's photo settings), the
  folding « Masquer / Afficher plus de comptes » card with the other accounts' initials, a « Déconnecté » session's
  « Connexion » / « Supprimer » and another instance's server pill with « Ouvrir » / « Supprimer », unread counters.
- **The app launcher follows the web's layout**: the favourites card and the other apps at the web's margins, the
  apps of a module with several apps (Office…) grouped under its name, everything sorted by name as on the web, and
  an « Administration » tile for administrators (it opens the console in the shell).
- The header buttons fit a normal title bar (`HeaderActions Compact`: 30-DIP buttons left of the caption buttons, the
  web's order, mirrored in right-to-left windows, an `AvatarTint` for accent-coloured title bars); an avatar follows
  its app's account data as soon as they arrive.
- **Documents and Chat have the app launcher and the account panel in their title bar.** The waffle and the avatar
  sit left of the window buttons and show the same apps, favourites and accounts as the shell and the web, for the
  account currently selected in Kubuno Desktop (borrowed from its token broker: these apps never hold a password or
  a refresh token). Editing the favourites saves them to the account; a tile opens Chat or Drive when they are
  installed, brings the running app forward for its own tile, and opens any other app in the browser; choosing
  another account switches every Kubuno app to it. The last apps, favourites and pictures are kept on disk, so the
  header looks the same offline and appears at once. `--sample`, and a Debug build started under a debugger, show
  sample data instead of the real profile. New shared crate `kubuno-header-data`, which now also holds the reading of
  the server's module list, the favourites' migration and the logo cache that the shell had on its own. Chat's title
  bar is back to the default height of a Kubuno window (it was a slimmer 32-pixel bar).

- **Views declare their XML namespaces** (`xmlns`, `xmlns:x`, and `xmlns:d` when design-time attributes are used, on
  the root element; vskubuno `docs/VIEWS-SPEC.md` §3): the shell's, Chat's and Documents' views, the examples and the
  test fixtures. The parser, the validator, the view compiler and the runtime treat `xmlns`/`xmlns:*` as markup, never
  as properties, and a view without them keeps working unchanged. The views language server notes an undeclared
  `x:`/`d:` prefix (an information, with a quick fix that adds the declarations) and completes the declarations and
  their URIs on the root element.

- **Kubuno Documents: the pages are editable.** Click to place the caret, drag (with auto-scroll), double- and
  triple-click, Shift+arrows, Ctrl+arrows, Home/End, Page Up/Down; type (accents, dead keys, AltGr, emoji, the IME
  window opens at the caret), Enter, Backspace and Delete across paragraphs and pages, Tab; undo and redo grouped like
  the web editor. Selections span page breaks. Cut, copy and paste keep the formatting between Documents windows
  (and as HTML and plain text with other applications); pasted images are resized. The ribbon's font, paragraph,
  style, find/replace, insert (page break, table, image, link, symbols, horizontal rule, code block) and margin
  commands now act on the selection, and their buttons show its state; a right click opens a Kubuno context menu.
  Dragging a margin, an indent or a tab stop on the rulers changes the document (one undo step per drag). At 50 % and
  below, pages sit side by side like on the web. Screen readers and the magnifier follow the caret.
- **Kubuno Documents opens and saves documents of the server** (`--doc <id>`), as the account the Kubuno shell shows:
  the access token is borrowed from the shell's broker, Documents never sees a password. It joins the editing session,
  saves automatically like the web editor (and on Ctrl+S), checks right before each save that nobody changed the
  document meanwhile and asks which version to keep when someone did (it never overwrites silently), stops and says so
  when a document is over the server's 2 MiB limit, and leaves the session on close. Unsaved work is kept on the PC
  first: after a crash or offline, Documents offers to restore it the next time the document opens.
- **`kubuno-docs-core`, the word processor's engine as a platform-neutral crate** (no Windows API; builds for
  Windows, Linux, macOS and WebAssembly): the document model, the `.kbdoc` envelope, the layout, line breaking and
  pagination ported from the web editor, hit-testing, the editing commands and the undo history. Text measuring is a
  trait the platform implements. The web editor could use it later through WebAssembly (vskubuno
  `docs/DOCUMENTS-EDITING.md` §4).
- **`kubuno-sync`: `request`, a generic authenticated request** of any method that carries the caller's headers
  (`If-Match`, `Idempotency-Key`) and returns the status, headers and body whatever the status.

- **A complete menu family for `.kbview` views** (vskubuno `docs/MENUS.md`). New elements `MenuBar` (Windows Forms
  `MenuStrip`, the web's workspace menu bar), `MenuSeparator`, `MenuHeader`, `DropDownButton` and `SplitButton`;
  `ContextMenu` and `MenuItem` gain `ToolTip`, `Command` (the ribbon's `Command`: one command for the ribbon, the
  menus and the shortcut), `ShortcutKeyDisplayString`, `ShowShortcutKeys`, item templates for bound menus and
  `OnClosed`. Menu shortcuts now run their item while the menu is closed, before the focused control; Alt or F10
  takes the menu bar from the keyboard, Alt + an underlined letter opens a menu, letters choose items inside a menu,
  the pointer moves from one open menu of the bar to the next, and Shift+F10 or the context-menu key opens the focused
  control's context menu. Menus are reported to screen readers (menu bar, menus, items with expand, toggle and
  invoke). A demo: `cargo run -p kubuno-views --example menus_demo`.

- **`kubuno-views-web`, the compiler of `.kbview` views for the web target.** A new platform-neutral crate
  (no UI, no Windows API; builds for every desktop OS and for WebAssembly) validates a view against the web
  element registry, produces the render plan the web runtime draws, and generates the TypeScript the web
  tooling needs (the `ViewBase` declarations of a view and the check file that lets `kbview-tsc` report
  binding errors at the `.kbview` line). It reuses the shared grammar (`kubuno-views-syntax`), so desktop and
  web views keep one parser. The core repository consumes it through the git tag `views-web-v0.1.0` and ships
  it as WebAssembly in `@kubuno/views-compiler`; the language server's web profile will reuse its generators.

- **Kubuno Documents is written like a Windows Forms application** and opens in the Visual Studio designer: its window
  is `document_window.kbview` (with `Application::run(DocumentWindow)`), its ribbon a declarative `<Ribbon>` with one
  `<Command>` per action (shared by the ribbon, the quick access toolbar and the keyboard shortcuts), its Backstage
  « Informations » a user control, its texts in `resources.kbres` (English) and `resources.fr.kbres` (French,
  `--culture fr|en`). The ribbon looks and folds exactly as before (same tabs, groups, menus, splits, gallery and
  tooltips); the commands that worked still work (zoom presets, ruler), the others are declared without effect, as
  before, but each now has an enabled/checked state the editing path can bind.
- **Rulers in Kubuno Documents, as on the web**: a horizontal ruler over the page (centimetre graduations from the left
  margin, grey margins around the white text column, Word's first-line, hanging, left and right indent markers hanging
  below it, the tab stops) and a vertical ruler graduating the page in view, with the tab-type selector in their corner.
  Drag a margin edge (a dashed guide crosses the page and a tooltip gives the margin in centimetres) or an indent
  marker, click to add or remove a tab stop: the page is laid out again live. « Affichage › Règle » shows and hides
  them, and the page moves up when they are hidden.
- **Zoom slider in the Documents status bar**: « − », a slider centred on 100 % (10–500 %), « + », and the percentage,
  which goes back to 100 %. The page also scrolls sideways (a horizontal scroll bar, Shift + wheel) when a high zoom
  makes it wider than the window, and `--dark` opens Documents in the dark theme.
- **Office ribbons in the dark theme follow the web's « Kubuno Dark »**: the tab strip (and the title bar that
  follows it) takes the dark window ground instead of the app's tone, the active tab and items the theme's accent,
  « Fichier » and the Backstage rail the web's lighter blue. In Documents the rulers and the ground around the pages
  are dark too (theme tokens; the paper stays white, as on the web).
- **Custom controls can name their parts**: `Control::cursor_at` and `Control::tool_tip_at` give the pointer shape and
  the tooltip over a point of a control (a ruler's markers), over the element's own `Cursor` and `ToolTip`.
- **`<StatusLabel ForeColor="…">`**: a status bar cell in its own colour (a warning in `Caution`).
- **`<RibbonComboBox ItemsSource="{Binding …}">`** fills the list from a bound list (fields `Value` and `Text`), and
  **`<RibbonGallery Display="Inline">`** is the web editors' single row of labelled chips.

- **The shell owns the accounts** (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md` §19.4): at start, Kubuno Desktop moves the
  tokens of the plaintext `creds.json` files into the Windows Credential Manager (machine-local, deleted only after
  the secret was read back; safe to interrupt and to run again), loads the accounts (server + user id) and becomes the
  only holder of refresh tokens on the machine: refresh rotation with the server's grace window, a session revoked
  elsewhere, account switch. File sync, chat and the other apps now borrow short-lived access tokens from it.
- **Token broker for the apps**: the shell serves a named pipe restricted to the current user (remote clients refused,
  only programs installed next to the shell accepted). An app whose shell is not running starts it in the background
  (`--background`) and waits for it. Chat now signs in through the shell: it no longer needs a sync folder, only a
  signed-in account.
- **Two-factor sign-in**: when the server asks for it, the sign-in page asks for the authenticator code (or a backup
  code) in place of the password.
- **Signing out with unsent changes**: « Envoyer d'abord » (the default: the changes are sent, then the account is
  signed out), « Exporter » (the files of the unsent changes are copied to a folder of Documents first) or
  « Supprimer quand même ». Signing out the last folder of an account also deletes its secrets and local data.
- **A session revoked from elsewhere pauses the sync** (nothing local is deleted; an activity entry and a notification
  ask to sign in again) and the sync resumes by itself after signing in again to the same account.
- **Sandboxed profile** (`KUBUNO_SANDBOX_DIR`): runs the shell and the apps for real with every file, secret and
  broker pipe of their own and no system registration (`Run` key, Explorer, Cloud Files, `kubuno://`), for tests and
  demos. `kubuno-sync add --server … --folder …` adds a sync folder to an account signed in in the shell (the CLI's
  own `login` is gone).

- **More of WPF's binding grammar** (`kubuno_views::binding`, vskubuno `docs/VIEWS-SPEC.md` §6.1): `Mode=OneTime`
  (read once, per item of a `Repeater`) and `Mode=OneWayToSource` (only writes; the property keeps what was entered),
  `UpdateSourceTrigger=PropertyChanged|LostFocus|Explicit` (a `LostFocus` write reaches the source when the focus
  moves, an `Explicit` one when the code calls `binding::update_sources`; the property shows the pending value
  meanwhile), `FallbackValue` (what the property shows while the path does not resolve), and `Converter` /
  `ConverterParameter` with built-in converters (`Not`, `IsEmpty`, `IsNotEmpty`, `ToUpper`, `ToLower`, `Trim`,
  `Equals`, `NotEquals`, `BoolToText`, `Count`) and the application's own: `#[kubuno_views::value_converter]` on an
  `impl ValueConverter for T` (or `register_converter`) registers it before `main`.
- **The language server knows what a binding can name** (`kubuno-views-ls`): `kubuno/bindingSources` answers the
  source schema of an element — its view's data context (a form class's `#[bind]` fields and `#[data_context]` paths,
  a user control's properties, a hand-written `impl ViewModel`'s arms, with their Rust types and locations), the row of
  the template it is in (the `d:ItemsSource` sample's keys, a binding source's columns, the code-behind's
  `Row::new().with(…)` chain), the data components and their members, the `.kbres` resources and the converters.
  On top of it: completion of keys, paths, sources, modes, triggers and converters; hover; go-to-definition to the
  Rust member; warnings for an unknown key, mode, trigger, converter or path, a type that does not fit the property and
  a two-way binding of a read-only member; the quick fix « Mettre à jour les liaisons » after a rename;
  `kubuno/bindingPreview` (a sample value through a binding's converter and format) and `kubuno/bindingDefinition`.

- **Tolerant compilation for the designer** (`kubuno_views::tolerant`, `Runtime::reload_for_design`, vskubuno
  `docs/DESIGNER.md` §17): a view with errors still renders everything valid in the design surface. Unknown elements
  and children in the wrong parent become hatched placeholders at their place (children built inside, element ids
  unchanged); unknown attributes, invalid values and malformed bindings are ignored with a warning marker; extra
  children are not shown; a mismatched closing tag is mended. A text that is not well-formed keeps the last good
  preview (dimmed); a broken file opened first is rebuilt from what the parser recovered. The surface reports a
  `renderStatus` (state and diagnostics, UTF-16 positions) and a `goToSource` on a marker click; the validator pairs
  each finding with its repair (`validate::validate_with_repairs`).
- **Messages in the user's language** (`kubuno_views::messages`): the parser's, validator's and builders'
  diagnostics and the design surface's texts are translated into French when `KUBUNO_UI_LANG=fr` (the language
  server and the design surface); identifiers are kept.
- **« Did you mean » suggestions**: an unknown element or attribute close to a known one names it, and a
  XAML-style `Binding="Name"` suggests the element's `Text="{Binding …}"`; the language server offers the quick fixes.

- **User controls are `.kbcontrol` files** (vskubuno `docs/VIEWS-SPEC.md` §1.1): a view whose root is
  `<UserControl>` now uses the `.kbcontrol` extension, forms and dialogs keep `.kbview`. The language server
  (`kubuno-views-ls`) reads both, and warns when a file's extension disagrees with what it holds — a user control in a
  `.kbview` (by its root or its `#[derive(UserControl)]` code-behind), a form in a `.kbcontrol` — with the quick fix
  « Renommer en .kbcontrol » / « Renommer en .kbview » that renames the file and updates the code-behind's
  `#[user_control(view = …)]` / `#[kubuno::view(…)]` and the `x:Inherits` of the views deriving from it. Chat's
  `conversation_list_pane`, `conversation_pane` and `conversation_row` and the user control test fixtures are now
  `.kbcontrol` files.

- **Offline-first data sync, foundation** (vskubuno `docs/DESKTOP-OFFLINE-SYNC.md`, lots SE-0 to SE-3; libraries
  only, not yet used by the shell or the apps): four cross-platform crates in `common/`, in the solution under
  *Libraries*.
  - `kubuno-secrets`: secrets in the OS credential store — Windows Credential Manager (local to the machine, never
    roaming), macOS Keychain, Linux Secret Service — with an in-memory store for tests and an opt-in owner-only file
    for Linux sessions without a Secret Service.
  - `kubuno-api-client`: a typed client of the Kubuno web API — any HTTP method, `If-Match` and `Idempotency-Key`
    headers, cursor paging of the Kubuno Delta Protocol v1, retries with backoff that honour `Retry-After`, server
    errors classified (retry later, conflict, rejected, session expired).
  - `kubuno-account`: accounts identified by server and user (stable across data wipes), sign-in with the two-factor
    (TOTP) step, the token owner (one refresh at a time, rotation persisted before use, recovery of a lost refresh
    answer, expired sessions reported without losing local data, account switch), a local token broker (named pipe on
    Windows, Unix socket elsewhere, current user only) through which apps borrow access tokens without ever seeing the
    refresh token, and the migration of the plaintext `creds.json` into the OS credential store.
  - `kubuno-sync-engine`: a local SQLite database per account and app, encrypted with SQLCipher (key per account in
    the OS store); local changes saved instantly and queued with one idempotency key per change, sent once when the
    network is back; changes rejected by the server rolled back visibly; server changes pulled page by page in
    crash-safe transactions, with pending local changes kept on top; conflicts merged field by field or recorded for
    the user (keep mine / keep the server's / keep both); a scheduler (startup, interval, local edits, server hints,
    network back, wake from sleep) and a sync status for the UI; per-OS data folders (`%LOCALAPPDATA%\Kubuno`,
    `~/Library/Application Support/Kubuno`, XDG).
- Build: SQLCipher requirements (a native Perl and preferably NASM on Windows) documented in `BUILD.md`.

- **Kubuno Desktop (the shell) written like a Windows Forms application** (vskubuno `docs/DESKTOP-MIGRATION.md`, app
  lot 2): `Application::run(ShellWindow::new(…))`, the window designed in `shell_window.kbview` and one user control
  per page — `LauncherPage`, `SettingsPage`, `AccountsPage` (a Repeater of `AccountRow`), `ActivityPage` (a Repeater
  of `ActivityRow`), `LoginPage`, `LabelsPage` (a Repeater of `LabelRow`) — plus the `StatusPresenter` (loading,
  empty, failed), the in-window confirmation dialog and two custom controls, `StorageGauge` and `StatusDot`. The
  64-DIP header is the window's title bar, built from title-bar regions: the gaps between its controls drag the
  window, the controls do not. Every view opens in the Visual Studio designer with sample data; the texts are in
  `resources.kbres` (English) and `resources.fr.kbres` (French).
- Shell: the app launcher and the account panel are flyout windows designed in `waffle_flyout.kbview` and
  `user_flyout.kbview`, frosted like the web's (a blur of what is behind them, rounded at 28, a drop shadow). The
  launcher's tiles are the `AppTileGrid` custom control: a click on a favourite removes it while editing, a click on
  another app adds it, and a tile dragged onto a favourite lands in front of it. The account panel's cards are
  `PanelMenu` controls and « Gérer votre compte » an `AccentPill`. Both panels follow the dark theme.
- Window kinds: `CornerRadius` on a `WindowKind="Flyout"` view shows it as a floating panel (`host::FloatingPanel`):
  Windows.UI.Composition blurs what is behind the window and clips it to that radius, the panel's drop shadow falls in
  a margin around it, the view's translucent `BackColor` tints the blur, and a click in the margin goes to the window
  under it.
- `Form::set_client_size` resizes a window that is already open (WinForms' `ClientSize`), through
  `host::resize_page`.
- Custom controls: `Control::accessible_parts` lists what a control paints itself (a grid's tiles, a menu's rows) in
  the accessibility tree, under the control.
- **`kubuno-desktop.exe --sample`**: the offline sample — a fixed account, launcher, labels, activity log and
  administration data, nothing read from or written to the configuration, no tray icon, Explorer entry, `Run` key or
  sync. `--light` / `--dark` force the theme, `--culture fr|en` the language, and `--page login` opens the sign-in
  page. For screenshots, demos and tests. A Debug build started under a debugger (F5 in Visual Studio) runs the sample
  by default, so a debugging session never touches the real profile or the system integration; `--live` opts out.
- Views: `TitleBarPadding` on a window (`Form::set_title_bar_padding`), the title bar's side insets (16 by default).
- Views: `CheckAlign` on a `Switch` (Windows Forms' `CheckBox.CheckAlign`): `MiddleRight` puts the switch at the right
  end of its box, after its label — a settings row.
- Views: `Solid` on a `Badge`: the saturated colour of its variant with white text, a counter pinned on a button.
- Views: `Tint="Primary"` on an `Avatar` (the accent colour, white initials: the signed-in user in a header).
- Views: `Role="Page"` and `Role="PageAdmin"` on a `Label` (and `Role::Page`/`Role::PageAdmin` in `kubuno_ui`): the
  page-title steps of the web scale (22.5, and 27.5 in the administration console). The shell's page titles use them.
- Views: an `Indent` field in a `Sidebar`'s `ItemsSource` rows, and an `Indent` property on a written `SidebarItem`,
  in pixels — flat rows that line up with a tree's groups.
- Views: `ActiveControl` of a user control moves the keyboard focus (`set_active_control(Some("server"))` focuses the
  control of that name inside, at the next frame), as in Windows Forms.
- A `Sidebar`'s rows are in the accessibility tree (its headers as texts, its rows as list items, by their labels).
- Shell: the administration console is a user control per section (`AdminPage` hosting `DashboardSection`,
  `UsersSection`, `GroupsSection`, `AudiencesSection`, `OrgUnitsSection`, `ModulesSection`, `InstanceSettingsSection`
  and `StorageSection`), each opened by the shared `AdminSectionHeader` (breadcrumb, title at the console's page size,
  count, introduction). The users list is a `DataTable` paged by the server. The custom controls `StatCard`,
  `BarChart`, `OrgUnitTree` and `StackedBar` draw the dashboard's figures, the units' tree and the storage's
  composition bars. `--page admin:<section>` opens a section directly again, and the rail's console group opens on
  the active section.
- Views: `AutoSizeMode="Fill"` on a `DataTable` `Column` (WinForms' `DataGridViewAutoSizeColumnMode.Fill`): the column
  takes the width the others leave, its own `Width` at least; when that does not fit, the table scrolls sideways.
- Views: `OnCellClick` on a `DataTable` (`CellEventArgs`: the row and column clicked).
- Views: `ItemHeightField` on a `Repeater`: each row names its own item height (cards as tall as their content).
- Views: a `BreadcrumbItem`'s `Text` can be bound, and a `Column`'s `Header` can name a resource (`{Res key}`).

- **User controls, like Windows Forms' UserControl** (vskubuno `docs/EVENTS.md`, "User controls as built"):
  - a user control's `Load` runs in the designer of the views that use it too, with `design_mode()` true, so it can
    show sample data there. Its own view's `<UserControl … OnLoad="…">` handler (a method of the user control) runs
    first, then `on_load` and the `Load` of the element using it. A panic in that code is shown in the control's box
    in the designer instead of ending the design surface. Repeater items load the same way;
  - colour properties: `#[property] accent_color: Option<ColorValue>` (or `ColorValue`, or a fixed `Color`) gets the
    Properties window's colour editor and accepts theme colours, `#RRGGBB`, web and system colour names;
  - string-list properties: `#[property] countries: Vec<String>` gets the String Collection Editor and is written
    one item per line;
  - `#[property(on_change = "update_bar")]` calls a method of the control after the property is set (from an
    attribute, a binding or a Repeater row), the body of a Windows Forms property setter;
  - `#[toolbox(bitmap = "address_editor.png")]` (or an `icon` naming an image file) gives a project control its own
    Toolbox image, Windows Forms' `[ToolboxBitmap]`. The image sits next to the source file and is checked at compile
    time;
  - code first: `Custom::<AddressEditor>::new()`, `Custom::<AddressEditor>::init(|e| e.street = "…".into())` and
    `Custom::from_instance(…)` create a user control or custom control of the application in code, with
    `.location()`, `.size()`, `.anchor()`, `.dock()`, `.property()` and `.on::<Args>("OnEvent")`, ready for
    `form.controls().add(&editor)`;
  - a user control dropped from the Toolbox gets the size it was designed at (its `DesignWidth` × `DesignHeight`),
    and the registry export gives it (`design_size`);
  - the `Modifiers` property offers `Protected` and `ProtectedInternal` too;
  - **design-time data**, XAML's `d:` attributes: `d:Text`, `d:Visible`… apply in the designer only (a control bound
    `Visible="{Binding HasError}"` can be shown while designing). `d:ItemsSource` gives a Repeater its sample items,
    as a JSON array inline or in a file next to the view. Without it, the sample values follow each bound field's
    name and type: initials, counts, booleans, colours, times, dates.
- **Visual inheritance** (Windows Forms' inherited forms and inherited user controls): the root of a view may name a
  base view, `x:Inherits="base_form.kbview"`. The view then shows every control of the base, may change the
  properties of the controls the base makes `Protected` or `Public` (an element of the same `x:Name` at the same
  place), and adds its own. The base's private controls stay as they are and are drawn with a lock in the designer,
  where they cannot be selected. `#[kubuno::view]` merges the views at compile time. A `#[base] base: BaseForm` field
  gives the derived form its base form: its controls (`self.base.ok`) and its event handlers, which keep running.
  `#[derive(UserControl)] #[kubuno(extends = AddressEditor)]` is an inherited user control: it keeps the base's
  properties, bindings, events and handlers. Chains of inherited views are merged, a loop is an error, and so is
  changing a private control.
- The user control's view in the designer is a plain surface (no window frame or caption buttons), like the Windows
  Forms UserControl designer.
- **Chat, rebuilt on the Windows Forms-like model** and editable in the Visual Studio designer: the window
  (`chat_window.kbview`), the conversation list, a conversation row and the open conversation are `.kbview` views
  with their Rust code-behind, and the messages are a new `MessageThread` control (in the Toolbox under
  "kubuno-chat Composants"). The window now has a slim title bar with the app icon and « Kubuno Chat », and a
  messenger layout: a navigation rail (Chats, Meetings; Settings and Account at the bottom), the conversation list
  with its search, and the conversation.
- Chat: the **Send** button and **Enter** send the message, **Shift+Enter** starts a new line; the message box
  grows with its text (up to six lines) and has real caret, selection, undo and clipboard editing.
- Chat: the **search field filters** the conversations (name and last message, accents and case ignored); the
  rail's **Meetings** shows the meeting conversations only.
- Chat: the conversation header's **search** filters the open conversation's messages; **details** (ⓘ) and
  **more** (⋮) open menus: copy the conversation's `kubuno://` link or id, mark as read, close the conversation.
- Chat: **Settings** switches the light / dark theme and French / English live; **Account** shows the
  connected server (or that the sample conversations are shown) and quits.
- Chat: messages show a separator per day, and a double-click copies a message's text. `--sample` shows the
  offline sample conversations only (also with `--dark`, `--culture fr|en`).
- For application developers: a multi-line text field whose Enter submits (`AcceptsReturn="false"`) now breaks the
  line on **Shift+Enter**; a `{Res key}` on a `<MenuItem>`'s `Text` is resolved; a horizontal `<Stack>` measures
  as tall as its tallest child (so an auto-growing `TextArea` grows a docked or auto-sized row); setting a bound
  `TextArea`'s text from code replaces it even while the field has the focus.

- **Splash screens.** Kubuno Desktop (the shell), Drive, Chat and Documents now open with a large splash
  screen (800 × 500): an original, procedural artwork per application in the colours of its logo — the Kubuno
  aperture rings and floating cubes, Drive's storage slabs and data streams, Chat's ripples and speech bubbles,
  Documents' fanned pages and ribbon — around a large rendition of the module's mark, with the product name, its
  version, a live status line that follows the real start-up steps, a thin progress bar, and the licence and
  credits. It appears within a few tens of milliseconds of the launch, fades in, stays at least 1.5 s, and fades out
  into the main window once that window is on screen (a click dismisses it). It is drawn at the launch monitor's
  scale (sharp from 100 to 200 %), on the monitor the application was started on, on top of other windows without
  taking the focus, and keeps the same dark look in the light and dark themes. Screen readers announce the
  product and each start-up step.
- No splash screen when it is not wanted: the shell started at logon, a Chat launch that hands a link over to the
  running Chat, a Drive tab torn out into a new window. `--no-splash` on the command line, the `KUBUNO_NO_SPLASH`
  environment variable, or `SplashScreen` = 0 (DWORD) under `HKCU\Software\Kubuno\Desktop` turn it off for every
  application.
- For application developers: `kubuno::SplashScreen::new().artwork(..).product(..).version(..).show()`, then
  `splash.set_status(..)`, `splash.set_progress(..)`, `splash.close_when(&main_form)`, `min_duration`, `dismiss`,
  `time_to_first_paint`; `kubuno::splash::from_kbview` sets it up from a splash view designed in Visual Studio,
  whose new `<SplashArtwork>` element draws the same artwork in the designer.

- **Resources and localisation** (`vskubuno/docs/RESOURCES.md`), the equivalent of Windows Forms' `Resources.resx`:
  `.kbres` files hold strings, images (SVG, PNG, JPEG, BMP, GIF, ICO, TIFF, WebP), icons, sounds, files, colours and
  fonts, linked to a project file or embedded, each with a comment; `resources.fr.kbres`, `resources.de-DE.kbres`
  translate them. `kubuno::resources!("resources.kbres")` generates a typed `Resources` class
  (`Resources::welcome_text()`, `Resources::logo()`, `Resources::app_icon().sizes()`, `Resources::ding().play()`,
  `Resources::accent()`), checked and embedded at compile time — nothing is read from disk at run time.
- **UI culture**: `kubuno::resources::culture()` follows Windows' display language; `set_culture("fr")` switches the
  whole application live (fallback `fr-CA` → `fr` → another `fr-*` → neutral), and `on_culture_changed` notifies.
- **`{Res key}` in views**: `Text="{Res welcome_text}"`, `Image="{Res logo}"`, `BackgroundImage="{Res banner}"`,
  `Icon="{Res app_icon}"`, `ForeColor="{Res accent}"`, on every bindable property, refreshed when the culture changes;
  `Source=` picks a resource file. The language server completes, documents, navigates to and checks resource keys.
- `kubuno-resources-tool`: converts `.resw`/`.resx` files (one folder per culture, like drive-localization's 49
  locales) into `.kbres` files, and checks a resource file and its translations.
- **Icons from image files, everywhere an icon is shown** (`vskubuno/docs/ICONS.md`): an icon property accepts a name
  of the Kubuno icon set (`Icon="Save"`), an image file relative to the view (`Icon="resources/save.svg"`) — SVG, PNG,
  ICO, JPEG, BMP, GIF, TIFF or WebP — or a project resource (`{Res Logo}`). Icons are rendered at the exact pixel size
  they cover (SVG as vectors through Direct2D, raster images with a high-quality filter, the closest frame of an
  `.ico`), in buttons, menus, toolbars, sidebars, status bars, docked panels, empty states, the ribbon and the title
  bar. An SVG drawn in `currentColor` follows the control's colour and the light or dark theme.
- **How an icon is drawn**, on every element with an icon: `IconSize` (Small 16, Medium 20, Large 24, XLarge 32, or a
  size), `IconScaling` (Fit, Fill, Stretch, None) and `IconColor` (a theme colour or `#RRGGBB`, which recolours a
  one-colour icon). Buttons also place their icon by `TextImageRelation` (before, after, above or below the text,
  over it), `ImageAlign` and the new `IconSpacing`; above the text, the button grows to fit.
- **The window's icon from any image**: `Icon` on a view (or `Form::icon`) sets the title bar, task bar and Alt+Tab
  icon from an `.ico`, an SVG, a PNG or another image, or a glyph of the icon set.
- **Icons in code**: `Button::new().icon("Save")`, `.icon(IconSource::file("resources/save.svg"))`,
  `.icon(IconSource::resource("Logo"))`, with `.icon_size()`, `.icon_color()`, `.icon_scaling()`, and on buttons
  `.text_image_relation()` and `.icon_spacing()` (also on `IconButton`, `EmptyState` and `Icon`).
- The language server completes icon names with a picture of each icon and warns about an unknown icon name (with a
  suggestion), an unsupported image type or a missing image file.
- **The whole Lucide icon set** (1,700+ icons of lucide-react 1.18, plus its other names such as `CheckSquare`) is
  embedded beside the Kubuno themed icons and the module logos, and an icon name is now found by a hash lookup instead
  of a scan of the data.
- **A button's `Image`** is drawn by the same pipeline when the button has no `Icon` (SVG and every raster format,
  `{Res key}`), at its own size (an icon file: its 32 px image) unless `IconSize` says otherwise.
- **The ribbon as a family of designable controls** (`<Ribbon>`, `vskubuno/docs/RIBBON.md`): the ribbon and every
  element composing it — `RibbonTab`, `RibbonContextualTabGroup`, `RibbonGroup`, `RibbonControlGroup`, `RibbonBox`,
  `RibbonQuickAccessToolbar`, `RibbonBackstage` with `BackstageTab` / `BackstageButton` / `BackstageSeparator`,
  `RibbonButton`, `RibbonToggleButton`, `RibbonRadioButton`, `RibbonMenuButton`, `RibbonSplitButton`,
  `RibbonColorPicker`, `RibbonMenuItem`, `RibbonSplitMenuItem`, `RibbonCheckBox`, `RibbonComboBox`,
  `RibbonTextBox`, `RibbonNumericField`, `RibbonGallery`, `RibbonGalleryCategory`, `RibbonGalleryItem`,
  `RibbonLabel`, `RibbonSeparator` — are real controls of two new levels (`RibbonControl`, `RibbonItem`): each has
  its own properties (bindable), events and class, is the sender of its events, a typed field of the view
  (`self.bold.set_checked(true)`), a typed handle for code (`RibbonButton::new().label("Coller").on_click(…)`) and a
  base to derive from. The non-visual **`<Command>`** holds a label, icons, a shortcut and an enabled / checked state
  shared by every element whose `Command` names it, and raises `Execute`.
- **The ribbon engine** (`kubuno_ui::ribbon`) gains owned icon names, the rectangle of every tab, group, control and
  menu entry, contextual tab groups under a coloured header, check boxes, editable combo boxes, numeric fields, text
  boxes, joined control groups and boxes, a dialog launcher per group, a group icon for the folded chip, cascaded
  sub-menus, galleries as grids (in the ribbon with scroll and « more », as a drop-down, in a menu) with categories,
  a colour picker over the Docs palette, the quick access toolbar below the ribbon and its « add / remove » menu,
  KeyTips (Alt), size levels with `SizeDefinition` templates and per-tab scaling policies, a simplified one-row mode
  and a design mode. A ribbon using none of them (Documents) renders exactly as before.
- **Virtual regions** (`kubuno_views::virtual_regions`): a node that lays out its own sub-elements declares where each
  one is drawn, so the designer selects them and the input router raises their events on their own controls.
- Property elements (`<RibbonTab.ScalingPolicy>`) are accepted by the `.kbview` validator inside their owner.
- **A `.kbview` window's title bar takes the ribbon's colour**: when a `<Ribbon>` is used, the caption band continues
  its tab strip (tone, light or dark theme, live theme or `Tone` change), with a title and caption buttons in a colour
  that keeps WCAG contrast; it returns to the window's own colour when the ribbon is removed or hidden. An explicit
  `TitleBarBackground` wins, and `TitleBarFollowsRibbon="false"` on the view opts out. The designer frame shows the same.
- The ribbon in the designer: « + » glyphs and a smart tag on the selected tab, group or control (they open the
  designer's « Ajouter » and tasks menus); a group's `SizeDefinition` is a drop-down of `Auto`, `Custom` and the
  templates; `PreviewWidth` on `<Ribbon>` lays it out narrower on the design surface to preview how its groups shrink;
  a bound font / size combo shows its first choice instead of an empty field.
- Ribbon elements list only what means something for them in the Properties window (Width, Font, BackColor, Cursor,
  BackgroundImage… are hidden: the ribbon paints and lays them out).
- Ribbon handles' `small_icon` / `large_icon` take any `IconSource` (a name, an image file, a project resource).
- **Extending another app's ribbon** (`kubuno_ui::ribbon::merge`, `vskubuno/docs/RIBBON.md` §8): a module registers a
  fragment — groups added to an existing tab before / after a named group, or new tabs — for a ribbon by its
  `x:Name` (`ribbon.merge(fragment)` on the typed handle); the fragment's controls get their clicks back, and it goes
  away with its handle. The gallery's ribbon page shows an « Assistant » module merged into Accueil.
- `QatSettingsKey` on `<Ribbon>`: the commands the user adds to the quick access toolbar are saved per user and per
  application (`%LOCALAPPDATA%\Kubuno\settings\<app>\<key>.qat`) and restored at the next start.
- The language server warns about ribbons: a `<Scale>` naming no group of its tab or making a group larger again, two
  KeyTips that collide (among the tabs, or within a tab), a `Command` naming no `<Command>` of the view.
- A ribbon element's icon takes its `IconColor`, `IconSize` and `IconScaling`.
- `host::set_zoom`: a window's page drawn at a zoom factor (the host renders at its DPI × the factor); the `.kbview`
  design surface's zoom (`setZoom`: a factor, or « fit »).
- Programs built into `target\<profile>\` (and `examples\`) now start by double-click: the `kubuno-ui` link step also
  hard-links `kubuno_ui-<hash>.dll` (and its PDB) and Rust's `std-*.dll` next to them, keeping the three newest builds.
- A `dist\` folder with the release apps (`kubuno-desktop`, `drive`, `kubuno-chat`, `kubuno-documents`, `gallery`),
  their `kubuno_ui` and `std` DLLs, the command-line tools in `tools\`, debug symbols in `symbols\` and a `README.txt`.

### Fixed

- **Web views: generated files stay in `.kubuno/views`.** The views language server wrote the `.d.ts` and check files
  of a view outside the web project (or reached through `..`) outside `.kubuno/views`; they now go to
  `.kubuno/views/_external/…`, like the web build.

- **Drive: picking black as the background colour makes it visible.** The window tint starts fully transparent, and
  picking a colour is meant to make it opaque; that only happened when the pick changed the colour, so choosing the
  black chip (the transparent default's own colour) left the tint invisible. Any colour pick now makes a transparent
  tint opaque, while switching the colour model tab, the area shape or the harmony scheme still leaves it untouched.

- **Control libraries named `kubuno-…` work in views.** The `#[kubuno::view]` macro skipped every path dependency whose
  name starts with `kubuno` when it looked for an application's custom controls and user controls, so a library such
  as `kubuno-shell-controls` or a third party's `kubuno-acme-widgets` gave "unknown control" errors. Only the
  framework's own crates are skipped now, by an explicit list (`FRAMEWORK_CRATES` in `kubuno-views-meta`), which the
  views language server uses too; a test fails when a new `kubuno-*` crate of `src/crates` is not listed.

- **Clicks in a floating panel land where they are aimed.** In a window drawn with a shadow around it (the app
  launcher and account panels, and now any window with custom rounded corners), a click near a control's edge, the
  accessibility bounds read by screen readers and the menus opened from it were shifted by the width of the shadow.
- **The app launcher shows the same logos as the web.** The desktop drew its own vector copies of the module logos, transcribed by hand long ago, while the web had moved on to new artwork, and the Mail tile had no logo at all. The launcher now shows the files the web itself shows: the ones the server announces (downloaded, kept per account for offline starts), and, before the first download, the same files built into the app from the web's own sources at compile time, never copied by hand. Mail's tile wears the Mail logo like on the web, and the administration console's module list uses the same logos.
- **The launcher works offline.** Starting without the server shows the last apps and logos the server sent, instead of an empty launcher.
- **A profile photo changed on the web now shows on the desktop.** The header kept showing the photo it first loaded until the app was restarted; a new photo now appears at the next refresh, and the last one stays when the server cannot be reached.
- A hidden docked element of a panel no longer leaves an empty band in the panel's measured size (a collapsed card
  could leave room under the others in a scrolling panel).
- **Ribbon**: a `ScreenTipTitle`/`ScreenTipText` written as `{Res key}` showed nothing (it now follows the UI
  culture), an `<Option Label="…">` of a ribbon combo box lost its label, and a ribbon toggle running a `<Command>`
  kept a state of its own: it now shows the command's `Checked` (one state for every surface, a checkable command
  switches when it runs). The `Icon` of a `<BackstageTab>` or a `<RibbonGroup>` was ignored (a document icon was
  drawn instead), and the ribbon's tabs and controls were missing from UI Automation: they are now listed with their
  names and can be invoked (Narrator, test tools).
- **Signing in with a password works again.** A `<TextField PasswordChar="…">` (and a `<MaskedField>` with a password
  character) wrote its bullets back into its binding instead of what was typed, so the sign-in page sent « •••• » to
  the server and every password sign-in failed. The bound value is now always the real text; only the drawing is
  masked. Copy and Cut stay disabled in a password field (Paste and IME input still work), and assistive technology
  now sees a password field (UI Automation `IsPassword`) whose value is the bullets, never the password.
- **A window started hidden now gets `Load`.** A form opened hidden (`--background` at logon, a start by an app through
  the token broker, `start_hidden`, or `Hide()` before it ever opened) raised `Load` only once it was first shown, so
  the shell started in the background answered the apps but added no notification-area icon and started no sync until
  opened. As in Windows Forms, `Load` is now raised once when the window is created, shown or not (its first frame runs
  off screen at the window's created size), and `Shown` once, the first time the window is actually on screen; hiding
  and showing the window again raises neither a second time.
- `kubuno-ui` icons: the lucide rename alias table is now empty and its test asserts the real contract — the embedded set carries both the new and the old spelling of every rename the web uses (`CircleAlert`/`AlertCircle`, ...), so both resolve to themselves; an alias is only allowed for a name that is not embedded and must point to an embedded one.
- Controls now clip their children to their own box, as every Windows Forms control does. The elements of a user
  control's view no longer paint outside the instance using it when the view is larger than the instance (or an
  element is placed or anchored beyond its edge) — on a page, in a `<Repeater>` item (each item clips to its own
  box), inside another user control — and a `<Panel>`, `<GroupBox>`, `<Card>`, `<Stack>` or any other container no
  longer lets a child spill out of it. The clipped-away part is gone for the pointer too: it is not hovered, clicked,
  focused by a click, scrolled with the wheel nor selected in the designer, and the bounds given to assistive
  technology are cut to the visible part. A drag started on the visible part keeps the pointer outside it, as a
  captured mouse does. What a control deliberately draws just outside its own box (its focus ring, its shadow) is
  kept, the controls of the window's title band are not clipped to the page, and a `<Popover>`'s panel, menus,
  tooltips and drop-downs still float above the view. The designer still frames a clipped control's whole box.
- `kubuno/bindingPaths` no longer answers the paths of another view's `impl ViewModel` when the view's own code-behind
  has none (a `#[kubuno::view]` form class or a user control): it now answers the view's own data context.
- A binding's unknown `Mode` value (`Mode=Both`) and unknown keys were ignored silently; the language server now warns.
- Designer: `d:ItemsSource` gives a `DataTable` its sample rows, and a nested JSON array in a `d:ItemsSource` file is
  the list of an inner Repeater (it showed generated samples). An owner-drawn `DataTable` shows its default cells in
  the designer, which runs none of the view's handlers (it showed empty rows).
- Shell: the 24 user control views are `.kbcontrol` files (forms, the window, the dialog and the flyouts stay
  `.kbview`).
- Shell: every administration section shows sample data in its own designer (`design/*.json`, `d:` values).
- Views: an owner-drawn `DataTable` hands `OnDrawItem` the index of the bound row (`e.index`), also when the table is
  sorted or paged, instead of the index of the visible line.
- Views: a `DataTable` paged by the application (`TotalRows` set) no longer offers a page-size chooser it cannot honour.
- Views: a control with its own `Font` is measured in that font (`kubuno_controls::styled::measure_formats`), so an
  `AutoSize` label in a `Stack` is as wide as the text it paints instead of cutting it with an ellipsis.
- Views: a hidden docked control takes no band (Windows Forms): a `Dock="Left"` rail with `Visible="false"` leaves its
  width to the `Fill` page instead of an empty strip.
- Views: `d:ItemsSource` is read only by the designer; a running application no longer looks for the file (and no
  longer logs that it cannot find it).
- Windows: a window owned by a top-most window (a `TopMost` form's flyout or dialog) opens top-most too, instead of
  behind its owner.
- Custom controls: a property written `{Res key}` (`<AccentPill Text="{Res manage}"/>`) gets its resource, like a
  built-in control's property.
- Shell: the launcher and the account panel are readable in the dark theme (they kept a light panel under light text).
- Shell: in a narrow window (under 520 DIP) the header keeps only its buttons; the storage gauge and the brand's
  name no longer run under the menu button, the bell and the caption buttons.
- Designer: `d:Visible="false"` now hides a control on the design surface (the designer shows every other control
  whatever its `Visible`), so the pages of a window that switches pages no longer pile up on top of each other.
- Designer: a user control used inside another view shows the data its instance is given and its controls as they
  would run (like a Windows Forms user control on a form): its own `d:` samples apply only when that user control
  itself is designed, so the rows of a `Repeater` no longer all show the same sample.
- Accessibility: an element is named by what it shows — its text, a `{Res …}` text included, its placeholder, its
  description — else by its tooltip (an icon button's only words); the developer's `x:Name` is the last resort, and
  never for a decorative container or image. A static text's words are announced (they were empty). The elements of
  a user control and of a Repeater's items are in the tree under them (they were attached to unrelated parents).

- **Controls inside a user control or a Repeater item get their input**: the inner views were painted without the
  window's input router, so a custom control inside a user control (or a Repeater's template) never received its
  mouse, wheel or key events, and its handlers never ran. The inner elements are now routed like the page's, their
  handlers running on the user control first (then the item's row, then the page).
- **Context menus of a user control**: a `<ContextMenu>` declared in a user control's own view (or in a Repeater
  `ItemTemplate`'s) now opens on a right click of an element that names it and from code (`show_context_menu`), and
  its handlers run on the user control.
- **Accessibility of views with user controls**: the elements of a user control's view reused the page's ids. The
  accessibility tree then had duplicate nodes and AccessKit refused it, so UI Automation saw no element at all. Each
  instance's elements now have their own ids (and focus ids). The host also leaves duplicate node ids out of the tree
  and logs instead of failing when AccessKit panics while answering `WM_GETOBJECT`.
- A user control's properties set by the view using it are applied when they change only. They were re-applied
  every frame, so the user could not type into a field of the user control bound to such a property, and a `Load`'s
  sample data was overwritten at once.
- The `kubuno-views-macros` unit test of the class chains accepts a class derived from another class (the ribbon
  family).
- **Clicks on a user control**: a click on a user control's own surface raised nothing. Its view's root covered it,
  and no one raised its `Click`. The click now reaches the user control's `on_mouse_…` overrides (a right click that
  raises its own event, say). It also raises the `Click` of the element using it, then the handler the user control's
  own root names (`<UserControl OnClick="…">`). This holds on a page, inside a `<Repeater>` and for an `ItemTemplate`
  item. A click on a label of its view stays the label's, as in Windows Forms.
- A user control extending another user control of the project accepts the base's properties in a view (`Street` on
  a `FancyAddress` extending `AddressEditor` was an unknown attribute).
- The language server finds the controls of a library crate that depends on the `kubuno` facade only. Their
  Properties window and events tab were empty. It also finds controls declared anywhere in the package, not only
  under `src/` (a control added next to `Cargo.toml`).
- A double-click on an event in a user control's designer writes a typed method in its `#[event_handlers]` impl,
  named `<x:Name>_<event>`, instead of a free function `fn on_…(vm, value)`.
- A `d:ItemsSource` JSON file saved with a byte order mark (Visual Studio, PowerShell) gives the Repeater its sample
  items. It was ignored.
- Drive's « Background colours » flyout builds again against the redesigned colour picker (hue ring, area shapes,
  harmonies, model tabs with channel sliders, no alpha slider) and uses its shared hit-testing and activation. The
  tint's opacity is kept as it is; picking a colour while it is fully transparent (the default) makes it opaque so
  the choice is visible.
- Decoding an image whose codec is a Windows extension (WebP) no longer lets window messages through in the middle of
  a paint, which made the window's next resize fail and left it half painted.
- A contextual tab group's header stays readable when its colour is close to the tab strip's (a green group on the
  green Spreadsheet tone): it then takes a pale tint of its colour with dark text, like Office.
- The `.kbview` design surface no longer repaints four times a second while idle (it woke up on a 250 ms timer to
  read the host's messages; it now wakes when a message arrives), which could make the designed view flicker.
- **`<GradientField>`**, the web's gradient swatch: a 32×24 button showing the gradient that opens the
  `GradientPicker` (linear / radial, draggable stops, angle, per-stop colour, position and opacity, ✕) in a
  floating popover. Its `Value` is the web's own CSS serialisation (`linear-gradient(120deg, rgba(…) 0%, …)` /
  `radial-gradient(circle, …)`), two-way bindable, with `OnValueChanged`. Available in `kubuno_ui`
  (`color::GradientField`, `Gradient::from_css`), in `.kbview` views, as a typed `kubuno::GradientField` handle
  and in the Visual Studio Toolbox.
- **The colour picker's screen eyedropper**: the pipette button of the picker and of the quick swatch picker arms
  it; the next click anywhere on screen takes the colour under the pointer (Escape cancels), like the web's.
- **`<Repeater>`, a list whose items are views** (WPF's `ItemsControl`): each row of `ItemsSource` is shown by the
  `ItemTemplate` user control (an instance per item: its properties named like a row field are set from the row,
  its handlers are its methods) or by the element written inside the `<Repeater>`. Inside an item a binding reads
  the row, then the user control, then the page; a two-way binding to a row field writes
  `<ItemsSource>[<index>].<field>`; `current_item()` tells a handler which item raised it. Items keep their live
  tree (what was typed, the hover, the focus) while their `ItemKey` survives a change of the list; only the items
  in view are built (10 000 rows cost about 0.1 ms per frame). `Orientation`, `Wrap` (a grid of cards),
  `Spacing`, `ItemWidth`, `ItemHeight` (0 measures each item), `SelectionMode`, a two-way `SelectedIndex`,
  `ItemClick`, `SelectionChanged`, `EmptyText`; the designer shows `DesignItemCount` sample items whose fields read
  their own names.
- **Custom controls typed in a form's code**: `#[control] thread: Custom<MessageThread>` (the field is linked to the
  element of its name), `self.thread.with(|t| t.append(m))` / `with_ref`, and `Control::with::<T>()` on any handle.
  A custom control property may hold a list (`Rows`) or any Rust value (`Shared<T>`), set with a binding
  (`Messages="{Binding Messages}"`); the view model hands `Rows` / `Shared<T>` values (`#[bind]` fields accept both).
- **Custom controls of another crate**: a view names the controls of its package's path (and workspace path)
  dependencies like its own; the crate is referenced by the view so its classes register at start-up.
- **Menus**: `<MenuItem>` children make sub-menus; `Icon`, `Kind` (`Separator`, `Header`), `Danger`, bindable
  `Text`/`Enabled`/`Checked`/`Visible` (read when the menu opens), `CheckOnClick`, `RadioGroup` (with
  `CheckedChanged`), `ItemsSource` on a `<ContextMenu>` or a `<MenuItem>` (items from a list, reported by the menu's
  `ItemClicked` with their key), `DropDownOpening` to fill a sub-menu as it opens; `DropDownMenu` on `<Button>` and
  `<IconButton>` opens a menu below them; `show_context_menu` (and `Control::show_context_menu`) opens one from
  code. Left and Right move in and out of a sub-menu.
- **Navigation elements**: `<Sidebar>` (`<SidebarItem>` rows with icons, `<SidebarSection>` headers, expandable
  groups, `DisplayMode="Compact"` for the icon rail, rows from an `ItemsSource`, a two-way `SelectedItem`,
  `ItemInvoked` and `SelectionChanged`, arrows and Enter when it has the focus) and `<StatusBar>` with
  `<StatusLabel>` cells (`Spring`, separators, clickable cells with an icon).
- **`<Avatar>`** (a picture from a file or from bytes, else the initials on the web's colours or the accent tint, a
  circle or a rounded square, a presence dot) and **`<PictureBox>`** (`SizeMode` `Normal`, `Stretch`, `Zoom`,
  `Center`, `Cover`, `CornerRadius`, `BorderStyle`), both showing an image held in memory (`ImageData`) as well as a
  file.
- **`<Popover>`**: a floating panel anchored to a control of the view (`Target`, `Placement`, `Alignment`), shown
  while `IsOpen` is true and closed by a click outside it or Escape (`Opened`, `Closed`; `popover.show()`/`hide()`).
- **Layout**: `WrapContents` and `CrossAlign` on `<Stack>`, and `Stack.Fill="true"` on a child that takes the room
  the others leave; **`<TableLayoutPanel>`** with `ColumnStyles`/`RowStyles` (`Absolute`, `Percent`, `AutoSize`),
  `GrowStyle`, `CellBorderStyle`, `CellSpacing`, and the `TableLayoutPanel.Row`/`Column`/`RowSpan`/`ColumnSpan` of its
  children.
- The `ListSelected`, `ControlFillHover` and `TitleBarBackground` theme colours; `set_fore_color` /
  `set_back_color` on every control handle (no recomposition).
- `kubuno_controls::styled::load_image_bytes`: an image held in memory, decoded once per content.
- Typed handles `Repeater`, `Sidebar`, `StatusBar`, `Avatar`, `PictureBox`, `Popover` and `TableLayoutPanel` in the
  `kubuno` crate; `Rows`, `Row` and `Shared` in its prelude.
- The language server completes the element names of `ContextMenu`, `DropDownMenu` and `Target` values, the user
  controls of `ItemTemplate`, and a binding for a list or object property.
- **`<DataTable>` paging and states from the view**: `PageSize`, a two-way `PageIndex` and `OnPageChanged`;
  `TotalRows` pages on the server side (the bound rows are the page); `Density` (`Compact`, `Normal`,
  `Comfortable`), `Loading` (skeleton rows), `EmptyTitle`/`EmptyText`, `ErrorText` (the error state), and a
  declared sort `SortColumn`/`SortOrder`, written back by a click on a header.
- `ItemHeight` on `<ListBox>` and `<TreeView>`; `MinLines`/`MaxLines` on `<TextArea>`, which grows with its text
  between the two and scrolls past `MaxLines`.
- `AcceptButton` and `CancelButton` on a `<UserControl>`: Enter and Escape inside it click the named buttons.
- Rows of their own height in a `<TreeView>`: `Height` on an `<Item>`, or a `Height` field in the rows of its
  `ItemsSource` (the others keep `ItemHeight`).

- `kubuno_ui::library`: `module_path()` (the file the component library was loaded from, e.g. for diagnostics),
  `is_library_file_name()` and `FILE_STEM`.

- **A Visual Studio solution for the Windows workspace** (`windows/Kubuno.Desktop.slnx`, generated by the vskubuno
  extension's *Generate Visual Studio Projects*): one `.rsproj` next to each crate's `Cargo.toml`, the shell, Chat,
  Documents, Drive, `kubuno-views-ls` and `kubuno-data-tool` ready for Set as Startup Project and F5 with
  breakpoints, the libraries under *Libraries*, and the solution's SDK feed (`windows/NuGet.Config`,
  `windows/.kubuno/sdk-feed`). Every project builds the whole workspace, as `tools/build-all.ps1` does, so
  `kubuno_ui.dll` and the programs that load it always match; Test Explorer lists the workspace's tests. See
  vskubuno's `docs/GETTING-STARTED.md`, "Working on the Kubuno desktop apps".

- **The web's dock, on the desktop** (`kubuno_ui::dock::DockArea`, a port of `core/.../workspace/Dock.tsx`): panels
  as tabs around a work area, re-docked left or right, merged into a tab group, split above or below, torn off as
  floating windows (rolled up, maximised, snapped to the edges and to each other), columns and stacked groups
  resized, panels closed and reopened (a reopen button listing the closed panels), with the Visual Studio guide
  diamond and window-edge arrows, a single ghost rectangle showing where the panel lands, the tab menu (detach, dock
  left or right, close, reset the layout), double-click to detach or re-dock, and the web's « Material refined »
  look in light and dark. The layout is saved per key in the web's own JSON shape. Keyboard: Ctrl+Tab /
  Ctrl+Shift+Tab surface the next / previous panel, Escape cancels a drag or a resize, the Menu key opens the tab
  menu. A new *docking* page of the component gallery shows the PaintSharp and App builder layouts.
- **The workspace chrome** (`kubuno_ui::workspace`): `WorkspaceShell` (top bar with back, title, editor name,
  document details, search and delete; options bar; tool rail; bottom bar; status bar), `MenuBar` in the standard
  and PaintSharp styles, the standard Fichier / Édition / Affichage / Aide menus, and the dark, light and Office
  palettes.
- **Docking in views**: `<DockArea>` with `<DockPanel Title Icon Side Group Active Closable>` children hosting any
  control, one more child for the central area, `StorageKey`, a bindable `Layout` (JSON) and `ActivePanel`,
  `Theme`, `PanelsHidden`, and the `PanelActivated`, `PanelClosed` and `LayoutChanged` events; `<WorkspaceShell>`
  around it (`Back`, `Search`, `Delete`). In code, `kubuno::DockArea` offers `open`, `close`, `reset`,
  `save_layout` and `load_layout`. The designer shows the declared arrangement; a click on a panel selects its
  `<DockPanel>` and a control dropped on an empty panel becomes its content.
- **Kubuno windows look like the web's windows** (`kubuno_controls::window_chrome`): every Kubuno window — the main
  form of `Application::run`, dialogs (`show_dialog`), `MessageBox`, the print preview, tool windows, MDI documents,
  in-window dialogs and the gallery's `FloatingWindow` — now wears the band of the web `FloatingWindow`
  (`core/frontend/src/ui/FloatingWindow.tsx`, measured on the live web): 50 px accent band, 16 px insets, heading-size title, the window's icon,
  30 × 30 rounded caption buttons with the white veil on hover, square corners, the window shadow, a white content
  area. One painter draws it everywhere, so the running window, the designer and the gallery are the same pixels.
  - Native top-level windows: DWM caption/border tint, square or rounded corners, Mica / Mica Alt / Acrylic, snap
    layouts on the maximise button, resize borders and a resize grip, maximised insets, Alt+Space / right-click on the
    band / click on the icon open the window menu, Alt+F4, DPI changes.
- **Window properties, WinForms-complete and beyond** (on a view's root element, in the Properties window and in code):
  `WindowKind` (Form, Dialog, ToolWindow, Splash, Flyout, MdiChild — presets of the border, buttons and placement),
  `ShowIcon`, `HelpButton`, `SizeGripStyle`, `TransparencyKey`, `RightToLeftLayout`, `IsMdiContainer`,
  `SplashDuration`, `ResizeBorder` (a borderless window that still resizes), `Chrome`, `Backdrop`, `CornerPreference`,
  `BorderColor`, `AccentColor`, and a new *Title Bar* category: `TitleBarHeight`, `TitleBarBackground`,
  `TitleBarForeground`, `Subtitle`, `TitleAlignment`, `ShowTitle`, `CaptionButtonStyle` (Kubuno or Windows buttons),
  `CaptionButtons` (buttons of your own next to minimise) and `ExtendContentIntoTitleBar`.
- **Controls in the title bar and the action bar**: `TitleBar.Region="Left|Center|Right"` puts a control of the view's
  top level in the window's band (tabs, a search field, an avatar) — it takes the pointer instead of dragging the
  window —, `TitleBar.Drag="true"` makes any control a drag area (the way to move a borderless window), and
  `ActionBar.Region="Left|Right"` puts buttons in the window's action bar, the web dialogs' footer.
- **Window events**: `ResizeBegin`, `ResizeEnd`, `TitleBarDoubleClick`, `DpiChanged`, `HelpButtonClicked`,
  `CaptionButtonClick` and `MdiChildActivate` (with `FormClosing`, `Shown`, `Activated`, `Deactivate` as before).
- **Every kind of window**: tool windows (slim 32 px band, close only), splash screens (borderless, centred, top-most,
  closing themselves), flyouts (`show_flyout`, rounded, Acrylic, closing on a click outside), borderless windows,
  owned windows (`Form::set_owner`, `owned_forms`), **MDI** (`IsMdiContainer`, `set_mdi_parent`, documents drawn
  inside the parent in the floating-window look, drag, resize, minimise, maximise, `layout_mdi` Cascade / Tile /
  ArrangeIcons, `active_mdi_child`, `mdi_children`) and in-window dialogs over a veil (`show_in_window`).
- **`<FloatingWindow>`**: a window drawn inside a view, designable (Title, Icon, IsOpen, Modal, ShowClose, OnClose).
- **`MessageBox` in the Kubuno look**: the web `ConfirmDialog`'s layout — icon on a tinted disc, the message, text
  buttons in the action bar — with a new `MessageBoxIcon::Danger` for destructive confirmations. `<Icon Disc="…">`
  draws any icon on such a disc.
- `Application::set_theme` switches the open windows live (title bars, pages, dialogs); `Application::add_message_filter`
  (WinForms' `IMessageFilter`), `Form::on_message` (tray callbacks, `WM_COPYDATA`, `WM_SETTINGCHANGE`),
  `Form::start_hidden`, `Form::set_visible` / `hide`.
- Example `cargo run -p kubuno --example window_kinds [-- --open <kind>] [--dark]`: every kind of window.
- **Printing, the Windows Forms way** (new crate `kubuno-print`, `kubuno::printing`; vskubuno `docs/PRINTING.md`):
  - `PrintDocument` with `BeginPrint`, `QueryPageSettings` (change one page's orientation, paper or margins),
    `PrintPage` (draw the page on `e.graphics()` — the same `Graphics` as a control's paint — within `e.margin_bounds`,
    set `e.has_more_pages`) and `EndPrint`; `print()` sends the pages to the Windows print spooler as vector output
    (text stays text, fonts embedded). A view's `<PrintDocument>` prints through its `.kbview` handlers.
  - Printing into a file without any dialog (`PrintToFile` + `PrintFileName`): with "Microsoft Print to PDF" it
    writes a PDF, with the XPS Document Writer an XPS document; without a file name a Save dialog asks for one.
  - `PrinterSettings` (the installed printers, the default one, copies, collation, two-sided printing, page range —
    pages outside a chosen range are not sent —, the printer's paper sizes, trays, resolutions and colour support) and
    `PageSettings` (paper, tray, orientation, margins in hundredths of an inch, colour, the printable area).
  - `PrintPreviewControl` (zoom or fitted, 1 to 10 columns and rows of pages, start page, mouse wheel and Page
    Up/Down) and `PrintPreviewDialog`, a window in the Kubuno look with Print, Zoom, one/two/three/four/six pages, the
    page number and Close, in the application's light or dark theme.
  - `PrintDialog` and `PageSetupDialog`: Windows' own Print and Page Setup dialogs, bound to a document's settings.
  - Code-first: `let doc = PrintDocument::new(); doc.on_print_page(|_, e| …);
    PrintPreviewDialog::new().document(&doc).show_dialog(self);`. In a `#[kubuno::view]`, the printing components are
    typed fields (`self.print_document1.print()`, `self.print_dialog1.show_dialog(self)`); a document of the view
    printed or previewed from one of the view's handlers prints when that handler returns.
- UI Automation's *Invoke* (screen readers, test tools) now clicks the custom controls of a view too (a control class
  with an `OnClick` handler), not only the built-in buttons.
- The view language server (`kubuno-views-ls`) also finds the components of the libraries a project reaches through
  the `kubuno` crate (its `workspace = true` dependencies: `kubuno-print`, `kubuno-data`), so `<PrintDocument>` and the
  data components are known, completed and documented before the project is built.
- **Typed data sources** (`kubuno-data`, `kubuno::data`; vskubuno `docs/DATA.md` DATA-4): a `.kbdata` file describes
  a data source (connection name, tables, views, named queries) and `data_source!("shop.kbdata")` turns it at compile
  time into row structs (`Customer`) with `fetch_all`, `fetch_by_key`, `insert`, `update`, `delete` and one function per
  named query, checked by SQLx against the committed offline cache `.sqlx` — no `sqlx` dependency needed in the
  application. Typed rows load into and read back from a `BindingSource`; `*_task` variants run on the data runtime and
  are awaited from the UI. A stale or incomplete cache is a compiler warning at the `data_source!` call.
- **`DataTable` columns are formatted and editable in place, like a Windows Forms `DataGridView`**: `<Column
  FormatString="N2" Culture="fr-FR" NullValue="-" Alignment="Right" ReadOnly="true"/>` (and `Culture` on the table);
  F2, typing or a double-click edits the current cell, Enter/Tab/arrows commit and move, Escape cancels the cell then
  the row; edits reach the bound `BindingSource` (conversion and validation errors keep the row, with an error glyph
  in the cell); events `CellBeginEdit`, `CellValidating`, `CellValueChanged`, `CellEndEdit`. Header sorting sorts the
  values, not the formatted text. In the designer a bound grid shows its column headers over blank rows.
- `BindingSource.AutoFill`: fills the list when the window opens, without code (what the designer's drag and drop from
  Data Sources writes).
- `kubuno-data-tool`, the helper process of the Visual Studio data tooling (connections, schemas, queries, scripts,
  `.kbdata` generation, migrations, the SQLx cache), on the same drivers and secret stores as the runtime.
- `#[kubuno::view]` accepts the named data components of a view (`DbConnection`, `TableAdapter`, `BindingSource`,
  `ErrorProvider`, `BindingNavigator`, `DbCommand`).

- **`kubuno`, the one dependency of a desktop application, with a Windows Forms-like programming model**
  (vskubuno `docs/PROGRAMMING-MODEL.md`):
  - `kubuno::Application::run(MainView::new())` opens the main window at the view's designed size, with its window
    properties, Kubuno's title bar, the logs and crash reports routed to the debugger or the log file, and the view
    reloaded when it is saved in a debug build.
  - `#[kubuno::view("main_view.kbview")]` makes a struct the form of a view: a field per named control, typed with
    its control (`self.status.set_text(…)`, `self.hello.set_enabled(false)`), `initialize_component()`, and event
    handlers that are plain methods of the struct. The view is read when the application is built and embedded in
    it; no generated file is written.
  - Forms and controls built in code: `Form::new().text(…).client_size(…)`, `Button::new().text("OK").location(…)
    .size(…).anchor(Anchor::TOP | Anchor::RIGHT)`, `ok.click().subscribe(…)`, `form.controls().add(&ok)` (and
    `remove`, `clear`, `find`), mixed freely with designed views; what the user types is read back from the controls.
  - Several windows on one UI thread: `form.show()` opens another window, `form.show_dialog(owner)` a modal dialog
    owned by its window (the other windows do not take input meanwhile) that returns its `DialogResult`; buttons can
    carry a `DialogResult`; `MessageBox::show` / `show_with` show a message with buttons and an icon.
  - Custom controls (`#[derive(Component)]`, `#[derive(UserControl)]`) work in an application that depends on
    `kubuno` only.
- The host (`kubuno_controls::host`) supports several windows on one thread: nested modal loops
  (`HostOptions::owner`, `HostOptions::modal`, `run_scoped`) and modeless windows (`open_window`), each window
  keeping its own input, cursor, title bar and close handling; accessibility actions go to the window they are for.

- **Data access components (`kubuno-data`, vskubuno `docs/DATA.md`, lot DATA-1)**: a new crate that brings
  ADO.NET/WinForms-style database access to Kubuno desktop applications, PostgreSQL and SQLite first.
  - `<DbConnection>`: the connection string is never written in the view or the source — it is looked up by
    name in the environment (`ConnectionStrings__Name`), the Windows Credential Manager or the user secrets
    store (`%APPDATA%\Kubuno\UserSecrets\<id>\secrets.json`, the id coming from `[package.metadata.kubuno]
    user-secrets-id` in the application's `Cargo.toml`); `{secret:Key}` placeholders keep passwords out of a
    connection string, and a literal password is refused. Connection pools, a retry policy for temporary
    failures, TLS required by default for remote PostgreSQL servers, a warning when connecting as a superuser,
    `Schema` (PostgreSQL `search_path`, one schema per Kubuno module), and a `StateChange` event.
    `ConnectionStringBuilder` reads both the `Key=Value;` and URL forms and never displays a password.
  - `<DbCommand>` and `<TableAdapter>`: parameterized SQL only (`@name` parameters); a table adapter fills a
    table whose rows track their state (added, modified, deleted) and saves the changes with generated
    `INSERT`/`UPDATE`/`DELETE` statements in a single transaction — all or nothing, with generated keys written
    back and rows changed by someone else reported as a concurrency conflict.
  - `<BindingSource>`: current row and position, filter (`Name LIKE 'A%' AND Age >= 18`) and sort
    (`Name ASC, Age DESC`), add / edit / cancel / end edit, field validation (types, required values, lengths,
    `RowValidating`), and the `ListChanged`, `CurrentChanged`, `PositionChanged`, `CurrentItemChanged`,
    `AddingNew`, `DataError` events. What the user types in a numeric field that is not a number stays in the
    field and is shown as an error instead of being lost.
  - `<ErrorProvider>`: the errors of the current row and the last failed operation, for bindings
    (`{Binding Source=errors, Path=Email}`, `Email.HasError` for a field's `Invalid`, `Summary`).
  - All database work runs off the UI thread; an async handler fills or saves in one line
    (`kubuno_data::fill(&ui, "customers").await`). The components appear in the designer's component tray,
    Toolbox and Properties window like any non-visual component.
- `{Binding}` accepts the named forms `Path=` and `Source=` (`{Binding Source=customers, Path=Name}`), used to
  bind controls to a data component.
- **Data components owned by the view (vskubuno `docs/DATA.md`, lot DATA-2)**:
  - The view runtime now creates and owns the data components a view declares (and the named controls of
    libraries): bindings reach them by name without any code in the view model, `Runtime::components()` /
    `with_component::<BindingSource, _>("customers", …)` (and `kubuno_views::scope::current()` in a handler,
    `UiHandle::components()` in an async handler) reach them from code, and a hot reload of the view keeps
    their rows. Their `.kbview` handlers run — synchronously when a binding changed them, so an
    `OnRowValidating` handler can refuse a row and an `OnAddingNew` handler can cancel an addition.
  - `<BindingNavigator>`: the WinForms navigation strip (first, previous, position box, count, next, last,
    add, delete, save) with the Kubuno icons, disabled where an action is not possible; its Save item saves
    the binding source through its adapter (`AutoSave`) and raises `SaveItemClick`.
  - The `ErrorProvider` draws its error icon next to every control bound to a field in error — a red badge
    with an exclamation mark, the message as its tooltip, `IconAlignment`, `IconPadding`, `BlinkStyle` and
    `BlinkRate` as in WinForms.
  - Typed bindings: a numeric property bound to a number column gets a number, a check box a boolean, and
    `FormatString=` (`N2`, `C`, `P1`, `D5`, `#,##0.00`, `d`, `g`, `dd/MM/yyyy`…), `NullValue=` and `Culture=`
    (default: the Windows user's locale) format what is shown and parse what is typed back — `1 234,50` or
    `10/12/1815` in French. This applies to any binding: a text holding a number now reaches a numeric
    property, and a number shown in a text property is displayed instead of left empty.
  - `kubuno_data::fill`, `save` and the new `save_all` work on the view's components; `DataContext` keeps
    them outside a view (tests, tools).
- **Data access, second part (vskubuno `docs/DATA.md`, lot DATA-3)**:
  - MySQL / MariaDB (`mysql` feature, through sqlx) and SQL Server (`mssql` feature, through `tiberius`:
    Windows integrated authentication, TLS through SChannel) connections, with TLS required by default for
    remote servers.
  - Master/detail lists: `<BindingSource DataSource="customers" DataMember="customer_id = id"
    TableAdapter="ordersAdapter"/>` shows the current customer's orders, follows the master's current row, and
    a parameterized select (`WHERE customer_id = @customer_id`) reads only that customer's orders.
  - `save_all` saves several lists in one transaction in the right order: a new customer and its new orders
    are saved together, the orders taking the customer's generated key (temporary negative keys until then).
    `DbTransaction` runs any commands and adapter saves in one transaction.
  - Paging (`PageSize`, by offset or keyset, with the total row count; `PageIndex`, `PageText` bindings),
    optimistic concurrency (`ConflictOption`: compare the original values, or a row version such as SQL
    Server's `rowversion` or PostgreSQL's `xmin`), custom `InsertCommand`/`UpdateCommand`/`DeleteCommand`,
    stored procedures (`CommandType="StoredProcedure"`), cancellation of a running operation from any thread
    (`DataTask::canceller`) and the rows read by a running fill (`RowsRead`).

- **Debugging Kubuno applications in Visual Studio (vskubuno's `docs/DEBUGGING.md`)**:
  - Under a debugger, a panic in an application's frame (its event handlers, async handlers, views) is seen by the
    debugger first, like a .NET exception: the Kubuno crash window only opens if the developer continues past the
    break (the host catches the unwind at the end of the frame). A panic elsewhere on the UI thread breaks into the
    debugger before the crash window (`KUBUNO_BREAK_ON_PANIC=0` turns that off). Without a debugger nothing changes.
  - The debugger shows `Option`/`Result` with their payload (`Some("two")`, `Ok(42)`), `Mutex` values and the Kubuno
    types (`Value`, `Row`, `Sender`, `ElementRef`, `MouseEventArgs`, `Color`, `Rect`...) readably: `kubuno_views` embeds
    its natvis in every application's PDB.
  - `kubuno_views::debug_break()`, `kubuno_views::debug_break_on_error(result)` and `.break_on_err()` (in the prelude)
    break into an attached debugger - the `Debugger.Break()` of Kubuno, harmless without a debugger;
    `kubuno_views::debug::is_debugger_attached()`; `kubuno_ui::diagnostics::debug_break()`.
  - The UI thread is named "Kubuno UI thread" in the debugger's Threads window, the async `delay` helper thread
    "kubuno-delay".
- **Painting, owner-draw and drag and drop (EVT-8 of vskubuno's `docs/EVENTS.md`)**:
  - `kubuno_ui::graphics::Graphics`, a Windows Forms-like drawing API over the Kubuno canvas, drawn with Direct2D in the
    same frame as the widgets: lines and polylines, rectangles, rounded rectangles, ellipses, arcs, pies, polygons,
    Bézier curves, cardinal splines and `GraphicsPath` figures (even-odd/non-zero fill, hit test, flattening), solid,
    linear and radial gradient brushes, pens with width, dash style or pattern, caps, joins and inset alignment, text
    laid out in a box (alignment both ways, wrapping, character/word/ellipsis trimming, right-to-left, underline and
    strikeout) and measured, images (any WIC format, source rectangles, opacity), Kubuno icons, clipping to a rectangle
    or a path, transforms (translate, scale, rotate, matrices), `save`/`restore`, smoothing, text rendering,
    interpolation and compositing modes, `clear`. Every call can be recorded into a replayable display list.
  - Custom controls: `on_paint(e)` gets `e.graphics` (the `Graphics`, which still answers the canvas primitives, so
    existing paint code keeps working); `on_paint_background` paints `BackColor` (transparent over the parent's
    background with `SupportsTransparentBackColor`) and `BackgroundImage`; new `on_print` (off-screen rendering and
    printing), `paint_layers`, `invoke_paint`/`invoke_paint_background`, `draw_to_display_list`/`draw_to_bitmap`.
    `ControlStyles` are honoured: `UserPaint`, `Opaque`, `ResizeRedraw`, `SupportsTransparentBackColor` and
    `OptimizedDoubleBuffer` (`DoubleBuffered`: the paint is kept and replayed until the control is invalidated, resized,
    re-themed or its properties change). `invalidate()` asks for a frame. The `Paint` event lends the surface to its
    handlers (`PaintEventArgs::graphics()`), for custom controls, buttons and the new `<PaintBox>` element.
  - Owner-draw: `DrawMode` (`OwnerDrawFixed`, `OwnerDrawVariable` with `MeasureItem`) on `ListBox`, `ComboBox` and
    `Dropdown` (their edit field and drop-down list), `OwnerDraw` on `ListView`, `DataTable` (cells) and context menus,
    `DrawMode` on `TreeView` (`OwnerDrawText`, `OwnerDrawAll`) and `Tabs`, raising `DrawItem`/`MeasureItem` with the
    item's bounds, state, text and colours (`draw_background`, `draw_focus_rectangle`, `draw_default`).
  - Drag and drop over OLE: `AllowDrop` elements get `DragEnter`, `DragOver`, `DragLeave` and `DragDrop` for files from
    the Explorer, text and custom formats from any application or from the same window; `do_drag_drop(data, effects)`
    (`Control::do_drag_drop`) starts a drag (text, files, custom formats) and completes with the chosen effect.
  - The paint debug overlay: invalidated regions flash, layout bounds with padding and margin, and the frame time,
    toggled by `KUBUNO_PAINT_DEBUG` (`all` or `invalidate,layout,fps`) or live by the `Kubuno.PaintDebug` window message
    (Visual Studio's *Debug › Kubuno › Paint debug*).
- **Windows Forms-rich property sets on every control of a Kubuno view (EVT-7c of vskubuno's `docs/EVENTS.md`)**,
  inherited through the class hierarchy and honoured by the runtime, not only described:
  - `Control`: `BackColor`/`ForeColor` (a theme token such as `Primary` or `Surface`, which follows light, dark and
    high contrast, or a free `#RRGGBB(AA)`, web or system colour), `Font` (`Segoe UI, 12pt, style=Bold`), `Cursor`,
    `UseWaitCursor`, `RightToLeft`, `BackgroundImage`/`BackgroundImageLayout`, `Enabled` (disables the whole subtree),
    `Visible`, `TabIndex`/`TabStop` (the Tab order), `ToolTip`, `ContextMenu`, `AllowDrop` (`OnDragDrop` with the dropped
    files), `AccessibleName`/`AccessibleDescription`/`AccessibleRole`, `Margin`, `Padding`, `MinimumSize`/`MaximumSize`,
    `AutoSize`/`AutoSizeMode`, `CausesValidation`, `Locked`, `Tag`, `Modifiers`, `GenerateMember`.
  - `ButtonBase`: `TextAlign`, `Image`, `ImageAlign`, `TextImageRelation`, `UseMnemonic`, `UseVisualStyleBackColor`;
    `LabelBase`: `TextAlign`, `Image`, `ImageAlign`, `UseMnemonic`; `TextBoxBase`: `ReadOnly`, `MaxLength`,
    `AcceptsTab`, `PasswordChar`, `CharacterCasing`, `HideSelection`, `TextAlign` (plus `AcceptsReturn`/`WordWrap` on
    `TextArea`); `ListControl`: `Sorted`; `ScrollableControl`: `AutoScroll`; `ContainerBase`: `BorderStyle`;
    `CheckBox`: `CheckState`, `AutoCheck`, `ThreeState`; `RadioButton`: `AutoCheck`; `Slider`: `Minimum`, `Maximum`,
    `SmallChange`, `LargeChange`; `NumericField`: `Minimum`, `Maximum`, `Increment`, `DecimalPlaces`,
    `ThousandsSeparator`; `ProgressBar`: `Minimum`, `Maximum`.
  - The view's root element carries the window's properties: `Title`, `Icon`, `StartPosition`, `FormBorderStyle`,
    `ControlBox`/`MinimizeBox`/`MaximizeBox`, `ShowInTaskbar`, `TopMost`, `Opacity`, `WindowState`,
    `AcceptButton`/`CancelButton` (Enter/Escape click them), `KeyPreview`, `AutoScroll`, `MinimumSize`/`MaximumSize`;
    `Runtime::form_options` hands them to `HostOptions::form` and the host applies them live.
  - Mnemonics: `&Save` is shown as "Save", the S is underlined while Alt is held, and Alt+S activates the control (a
    label moves the focus to the next control).
  - New non-visual components `<ToolTip>` (`InitialDelay`, `AutoPopDelay`, `ReshowDelay`, `ShowAlways`, `Active`) and
    `<ContextMenu>` of `<MenuItem Text ShortcutKeys Enabled Checked OnClick>` (`OnOpening`).
- **UI Automation**: the host publishes the window's accessibility tree through AccessKit (created on first request by a
  screen reader), with the names, descriptions, roles, states and bounds of the controls, and routes its Click and Focus
  actions back to them.
- `kubuno_controls::host`: `FormOptions` (window style, layered opacity, topmost, window state, icon, minimum/maximum
  tracking size, start position, Kubuno caption buttons), `set_form`, `accept_files`/`InputEvent::FilesDropped`,
  `diagnostics::set_max_level`; `kubuno_controls::styled::StyledCanvas` (per-control theme, colours, font and
  decorations) and an image cache; `drive_app_controls::TextStyle`/`create_text_formats_styled`.
- `kubuno-views`: WCAG contrast checks (`validate::contrast_warnings`: a non-blocking warning on a free colour that
  does not reach 4.5:1 - 3:1 for large text - in the light or dark theme); property aliases (older names still accepted,
  reported as hints); the registry export lists each property's level (`inheritedFrom`), view-only properties
  (`rootOnly`) and category.
- `kubuno-views-ls`: completion and hover for inherited and view properties, contrast warnings, and the
  `kubuno/bindingPaths` request (the paths a view model's `get` answers).
- The designer surface draws the view's title and caption buttons as `FormBorderStyle`/`ControlBox`/`MinimizeBox`/
  `MaximizeBox` set them, resolves relative image paths against the view's folder (`setText` carries `baseDir`), and
  never moves, resizes or nudges a `Locked` control.

### Changed

- **Crates and Visual Studio projects renamed after the product they belong to** (`kubuno-<product>-<component>`, VS
  project `Kubuno.<Product>.<Component>`). The framework is now `kubuno-desktop` (the facade: applications write
  `use kubuno_desktop::prelude::*` and `#[kubuno_desktop::view]`), `kubuno-desktop-ui`, `kubuno-desktop-controls`,
  `kubuno-desktop-views*`, `kubuno-desktop-data*`, `kubuno-desktop-print`, `kubuno-desktop-resources*`,
  `kubuno-desktop-app-storage-components`, `kubuno-desktop-shell-controls` and `kubuno-desktop-header-data`; the
  cross-OS crates are `kubuno-desktop-account`, `-secrets`, `-sync`, `-sync-engine`, `-api-client`, `-app-storage`, and
  the word processor's engine is `kubuno-office-docs-core`. The web `.kbview` compiler is `kubuno-web-views-compiler-core`.
  The apps' packages are `kubuno-desktop-shell`, `kubuno-chat-desktop`, `kubuno-office-desktop` and
  `kubuno-drive-desktop` (with `kubuno-drive-desktop-app-controls`, `-app-storage`, `-core-storage`, `-shared` and
  `-localization`, the crates ported from Files). Folders follow the crate names.
- **Nothing changes for users**: the programs keep their names (`kubuno-desktop.exe`, `kubuno-chat.exe`,
  `kubuno-documents.exe`, `drive.exe`, `kubuno-sync`, and the tools `kubuno-views-ls.exe`, `kubuno-data-tool.exe`,
  `kubuno-resources-tool.exe`), the shell's settings stay in their `kubuno-desktop` folder, and the credentials,
  token broker, Run key, AppUserModelID and Linux package name (`kubuno-sync`) are unchanged.
- **One Visual Studio solution for the repository**, `Kubuno.Desktop.slnx` at its root, with the folders Applications,
  Framework, Shared controls, Common (multi-OS), Drive engine, Tools and Web; it replaces `windows/Kubuno.Core.Desktop.slnx`.
  The solution's SDK feed (`NuGet.Config`, `.kubuno/sdk-feed`) moved to the root with it.

- **Every app is a single, self-contained exe: the UI framework is linked statically.** `kubuno-ui` is now an
  ordinary Rust library linked into each program, together with Rust's standard library, instead of a shared
  `kubuno_ui-<hash>.dll` loaded next to `std-*.dll`. The shell, Chat, Documents, Drive, the gallery and the tools
  start from a folder that holds nothing but their own exe. Why: the apps will be released from their own
  per-module repositories, each on its own schedule, so they must not share a Rust DLL; Kubuno Desktop stays
  required on every PC as a service (account broker, sync, launcher), never as a binary dependency.
  Release build of the workspace (`cargo build --workspace --bins --release -j 1`, clean, this machine): 1318 s,
  was 1368 s. Exe sizes: `kubuno-desktop.exe` 24.3 MB (was 19.9 MB + the 16.0 MB DLL + 0.85 MB `std`),
  `kubuno-chat.exe` 20.5 MB (was 16.2), `kubuno-documents.exe` 22.3 MB (was 17.9), `drive.exe` 17.6 MB (was 14.6),
  `kubuno-views-ls.exe` 13.7 MB (was 10.4), `gallery.exe` 6.1 MB (was 1.6); the four apps together weigh about the
  same as before with the DLLs (84.7 MB, was 85.6 MB).
- The desktop shell (`shell.json`) and Drive (`settings.json`) keep their preferences with the shared settings
  engine; the existing files are imported once on first start (the old file is kept as `*.migrated`).
- The shell's header uses the shared `WaffleButton` and `AccountButton`; the launcher and the account panel look
  and behave exactly as before (favourites edit, drag to reorder, Escape abandons an edit first).
- The shared crate of the header menus is named `kubuno-shell-controls` (was `shell-controls`).
- The Mail tile and favourite are keyed `mail`, as on the web; favourites an older desktop saved as `mail-inbox` are
  read as `mail`.
- **The `.kbview` grammar and the element registry model are now platform-neutral crates** (vskubuno
  `docs/WEB-VIEWS.md` WV-1), so the web tooling (the WASM compiler, the Vite plugin, the language server's web profile)
  can reuse them without the Windows runtime. `kubuno-views-syntax` holds the lossless parser and syntax tree, the
  typed AST and element ids, the surgical edits (`match_line_endings` included), the `{Binding …}` and `{Res …}`
  grammars, the `x:`/`d:` markup attributes, the `.kbview`/`.kbcontrol` file-kind rules and the registry-independent
  core of the validator. `kubuno-views-model` holds the registry metadata types (`PropKind`, `PropertyMeta`,
  `EventMeta`, `ArgsChain`, `LevelMeta`, `ChildrenModel`, `LayoutKind`, the design-time attributes), the
  `kbview-registry.json` wire types with a new loader that reads the desktop export or the web registry back
  (`load_registry`), the Properties window editor kinds (`EditorKind`) and the binding source schema types. Both
  build for Windows, Linux, macOS and `wasm32-unknown-unknown`, with no UI or Windows dependency. `kubuno-views`
  and `kubuno-views-ls` re-export everything under the same paths as before: no code using them changes, and the
  behaviour is unchanged.
- **Text sizes now match the web app.** Every typographic role follows the web's current scale, at the same physical
  size: badges and counters 10.5, metadata and captions 11.5, body text (labels, fields, menus, tabs, buttons) 13.5
  instead of 12, section headers and window titles 15.5, panel titles 21.5 (was 11 / 12 / 12 / 16 / 22). Badges now
  have their own 10.5 face instead of borrowing the caption one, and a 22.5 page-title face (`TextFormats::page`,
  `metrics::text::PAGE`) is available. The workspace title, subtitle and document info, and the ribbon's Backstage
  rows, use the roles instead of fixed sizes. Message and empty-state paragraphs keep the web's relaxed leading over
  the larger body (22 DIP lines). The font stays Segoe UI Variable.

### Fixed

- A data table's empty state wraps its description in a narrow table instead of clipping it at both ends.

### Changed

- The shell no longer depends on Drive's crate (`drive-app-controls`): it uses the `kubuno` facade only (module
  isolation). Its hand-drawn frame, hit tests, hover state, focus ring and in-window dialog are gone, replaced by
  the views; the sync loop, the Cloud Files and Explorer integration, the tray, the toasts, `--page`, `--background`
  and the splash screen are unchanged. The geometry tests went with the code they tested; the shell has tests of its
  view models, its pages and its window instead.
- Views: a theme colour as a container's `BackColor` (`Background`, `Surface`…) only changes its own ground: the
  design system's surfaces inside it (a card, a white panel) keep their colour. A free colour (`#RRGGBB`) is still
  the Windows Forms ambient colour the children take.
- Views: Windows-style caption buttons (`CaptionButtonStyle="Windows"`) are 32 DIP tall at the top of a taller title
  bar, like Windows' own.
- Views: the letters of a large `Avatar` grow with it (a profile picture of 72 DIP or more).
- Forms: `Form::get_client_size()` follows the open window (Windows Forms' `ClientSize`); it is the design size until
  the window opens.
- Views: a control or user control of the application with an `x:Name` keeps its instance when the view is composed
  again (a control moved or resized from code, a hot reload): what code set on it through typed access stays.

- Chat: the message bubbles wrap on words at their real width (they used to break early on an estimate), and the
  conversation avatars use the design system's avatar colours.
- Chat: the "new message" and "attach a file" buttons are hidden until the chat service supports creating a
  conversation and uploading a file.
- **Start with Windows**: the shell now starts hidden in the notification area at logon (the `Run` entry carries
  `--background`; an entry written by an earlier version is updated at the next start), instead of opening its window
  in front of the user.

- **The Visual Studio solution of the desktop workspace is now `Kubuno.Core.Desktop.slnx`** (was `Kubuno.Desktop.slnx`), so it is clearly told apart from the web server solution `Kubuno.Core.Web.slnx` of the `core` repository.

- **The desktop colour picker now looks and behaves like the web's** (`ColorPicker`, 312 DIP): the hue ring with
  the saturation/value area inside it in its three shapes (square, triangle, circle), the six harmony schemes
  with their markers on the ring and their swatches, the preview and hex field, the RGB / HSV / HSL / CMYK / GRAY
  channel tabs with their gradient tracks, the twelve chips, the recent colours and the optional Cancel / Add
  footer — same geometry to the half-DIP, same colours in light and dark, same keys (the ring wraps, the area
  moves by 0.02 / 0.1, a channel by 1 / 10). The old stacked SV area with separate hue and alpha sliders is gone
  (the web's picker has no alpha: a colour's opacity is kept, not edited). The ring and the SV area are drawn
  pixel by pixel like the web's canvas, the tracks with real gradients.
- **`<ColorField>` opens its colour picker**: a click (or Enter / Space) opens the full picker in a floating
  popover placed like the web's (to the left of the swatch when there is room, kept on screen); a click outside
  or Escape closes it. The chosen colour is written back to a two-way `Color` binding and raises the new
  `OnValueChanged` event.
- The quick swatch picker (`SwatchPicker`) and the gradient picker match the web more closely: 6 DIP panel
  corners and the lighter shadow for the swatches, the web's chip outlines (a light grey ring on white, the blue
  selection ring), the « PERSONNALISÉ » caption size and spacing; the gradient's type buttons, rows and slider
  look (a thin track with a round thumb), the position box next to its caption, the bar drawn as a real gradient,
  and the ✕ only when the picker is in a popover.
- **Bound lists cost nothing per frame while they do not change**: `Value::List` holds `Rows`, a shared snapshot
  with a stamp, so reading a list every frame copies a pointer, and `ListBox`, `CheckedListBox`, `ListView`,
  `TreeView`, `DataTable`, `Dropdown` and `ComboBox` rebuild their items only when the list changed. With
  10 000 bound rows a frame went from 9.6 ms (`ListView`), 3.9 ms (`ListBox`) and 5.7 ms (`DataTable`) to
  30, 26 and 62 µs (`kubuno-views/tests/perf_lists.rs`). Keep a `Rows` in the view model and change it in place;
  code that built `Value::List(vec)` writes `Value::from(vec)`. `Value::Object` carries any Rust value.
- **Sub-menus nest to any depth**, and each open level is its own floating surface the size of its card (no
  surface around a menu and its sub-menu takes the clicks of what lies beside them); Left closes one level.
- **`<Popover>` paints above the whole view wherever it is declared** (it no longer has to be the last element), and
  while a light-dismissed one is open the view under it gets no click: a press outside only closes it, like a
  flyout.
- A click that a control inside a `<Repeater>` item handles (a button, a check box…) no longer also raises the
  item's `ItemClick` or selects it (WPF's `e.Handled`).
- The designer's canvas around the view follows the IDE's theme, live (`setCanvasBackground` in the designer
  protocol, `design::set_canvas_background`), with its scroll bars; the sample items of a `<Repeater>` give a
  number or a flag to a field named like a number or flag property of the item's user control.

- **Every build of the shared component library has its own file name, `kubuno_ui-<hash>.dll`, and every program
  loads exactly the build it was linked against.** A Kubuno program can no longer start on a library of another
  build and stop with « Point d'entrée introuvable » / "entry point not found" (`0xC0000139`): two builds live side
  by side in one folder or on `PATH`, a program rebuilt on its own no longer breaks the others (they keep running on
  their build), and when a program's library is missing Windows names it (`kubuno_ui-<hash>.dll` not found,
  `0xC0000135`). It works with plain `cargo build` / `cargo run` / `cargo test`, the Visual Studio projects, the
  debugger (the library's PDB is found beside it) and the view designer, without any setup (`kubuno-ui`'s build
  script, see `BUILD.md`). `tools/stage-runtime.ps1` copies next to each program the build it imports, with its
  PDB, and lists the programs whose build is gone; `packaging/package-msix.ps1` ships the build the packaged program
  imports; `tools/ui-parity/shoot.ps1` starts a program that was not staged.
- The design surface (`kubuno-views`' `view_embed` example) reports the library file it actually loaded, whatever
  its name, in its handshake with Visual Studio.
- The gallery's `FloatingWindow` and the dialogs built on it (`ConfirmDialog`, `PromptDialog`, `ConflictDialog`) now
  have square corners, like the web's windows (`--kb-window-radius: 0px`); a window can ask for rounded ones with
  `CornerPreference`.
- The Kubuno title bar is 50 px tall (it was 34), as the web renders it, and draws the web's caption buttons; windows using
  `Chrome::Kubuno` (Documents, the gallery, the view preview) get the new band.
- **Diagnostics log at `INFO` by default, Debug builds included**: the per-event `DEBUG` traces no longer flood the
  debugger's output; `KUBUNO_LOG=debug` or `diagnostics::set_max_level` brings them back.
- Attribute values decode XML character references (`&amp;` → `&`, `&#10;` → line break), and the designer's edits
  escape `&`, `<`, the quote, line breaks and tabs, so any text round-trips.
- `kubuno-views-ls` logs a notification it cannot read instead of exiting.
- The designer underlines mnemonics (`&Save`) all the time, as Windows Forms' designer does.
- Renamed properties, the old names still accepted: `Label.Align` → `TextAlign`, `Min`/`Max` → `Minimum`/`Maximum`
  (Slider, NumericField, ProgressBar), `Slider.Step`/`LargeStep` → `SmallChange`/`LargeChange`, `NumericField.Step` →
  `Increment`.

### Fixed

- **The designer boxed every control in light blue** (Labels looked like framed text, TextFields, grids and
  navigators had an extra frame): the paint-debug overlay's layout bounds reached the design surface when Visual
  Studio's *Debug › Kubuno › Paint debug* had been left on. A design surface now ignores the overlay entirely
  (environment variable and live toggle alike), and controls look exactly as they do at run time.
- In the designer, a container that paints nothing of its own (a `Panel` or `Stack` with no `Surface`,
  `BorderStyle` or background) gets a faint dashed outline in the theme's divider colour, like the dotted border
  Windows Forms shows around a borderless `Panel`, so it can be found and dropped into. Never drawn around
  controls, around a visible container or at run time; the surface's new `setDesignOptions` message turns it off.
- Designer edits (`kubuno/applyEdit`: drops, moves, deletes, data drops...) keep the view's own line endings: a
  fragment written with LF is inserted with CRLF into a CRLF file and vice versa
  (`kubuno_views::edit::match_line_endings`), so a view never ends up with mixed line endings.
- A custom control written from the `Control` level without its own `get_preferred_size` overflowed the stack as
  soon as it was painted (`Control::size` called itself through `ControlCore`).
- A program that had shown an image (a `PictureBox`, an `Avatar`, an icon file) exited with code 2170 instead of 0:
  the decoded images are now released before COM is shut down.
- The `<Splitter>` of a view can be dragged: a literal `SplitterDistance` follows the drag until the view changes it
  (a bound one is written back as before).

- `data_source!("shop.kbdata")` is found by rust-analyzer too: its proc-macro server gives no calling file, so the
  macro now falls back to the one file under `src` whose path ends with the given one (the error Visual Studio's Error
  List showed while `cargo build` succeeded); several matches are an error naming them.
- A bound `DataTable` without a `SelectedIndex` binding no longer loses its selection at the next frame, and sorting
  it by a header now holds.
- A failed fill (a missing connection string, a broken query) is reported once instead of being retried at every frame.
- The paint debug overlay's frame-time box ("3.2 ms · 4 fps") is drawn at the bottom-right of the client area, not at the
  top-right corner where it covered the minimize/maximize/close buttons of Kubuno applications.
- A race between two threads registering classes could leave the registry snapshot one class behind (a generation
  counter replaces the dirty flag).

- **Custom controls, user controls and components usable in Kubuno views (EVT-7b of vskubuno's
  `docs/EVENTS.md`)**. A `#[derive(Component)]` class of the application (`#[kubuno(extends = Control)]`, drawn by
  its own `on_paint`) is an XML element of its views (`<RoundButton CornerRadius="18"/>`): its `#[property]` fields
  are attributes, its `#[event]` fields events (`OnX`, raised with the generated `raise_x`), and the design-time
  attributes `#[category]`, `#[description]`, `#[default_value]`, `#[browsable]`, `#[default_event]`,
  `#[default_property]`, `#[toolbox(icon = …)]` describe it to the designer. `#[derive(UserControl)]` with
  `#[user_control(view = "rating_bar.kbview")]` makes a user control: a view of its own (root
  `<UserControl x:Class="RatingBar">`) whose code-behind is the control itself and whose events re-raise inner ones.
  `<Timer Interval Enabled OnTick>` is the first built-in non-visual component. Classes register themselves at
  start-up (no manual registration); a class the language server has seen but the program does not link yet is drawn
  as a named placeholder. `DesignMode` is true in the designer.
- **`kubuno-views-meta`**, the shared grammar of these declarations: the derive macros and `kubuno-views-ls` read the
  same metadata (the language server scans the project's sources without building it), checked by a golden test.
- **`kubuno-views-ls` knows the project's own controls**: completion, validation, hover and go to definition for them;
  new requests `kubuno/registryVersion` and `kubuno/crateComponents`.
- `view_embed --export-registry` writes the registry (with the linked project controls) as JSON; the example links the
  project crate when built with `--cfg kubuno_design_project`.

- **A Windows Forms-style control hierarchy for Kubuno views (EVT-7a of vskubuno's `docs/EVENTS.md`)**, in
  `kubuno_views::component`: `Component` → `Control` → `ScrollableControl` → `ContainerControl` → `UserControl` /
  `View`, with the family bases `ButtonBase`, `TextBoxBase`, `ListControl`, `LabelBase`, `ContainerBase` and
  `RangeBase`. Every built-in control is now a class of this hierarchy (`kubuno_views::controls::Button`,
  `TextField`, `Panel`…; the structural elements such as `<TabItem>` are non-visual components), and every element of
  a `.kbview` view is backed by an instance of its class.
  - **Custom controls by extending a built-in one**: `#[derive(Component)] #[kubuno(extends = Button,
    overrides(Control))] struct RoundButton { base: Button }` inherits everything a button does; its
    `impl Control for RoundButton` overrides only what it changes. `self.base_mut().on_click(e)` calls the base
    behaviour, like `base.OnClick(e)`. Trait objects upcast (`&dyn ButtonBase` → `&dyn Control` → `&dyn Component`)
    and downcast (`downcast_ref`, `is_a("ButtonBase")`, `find_base::<Button>()`).
  - **Overridable `on_…` methods** whose base behaviour raises the matching event, so overriding and subscribing
    compose: `on_paint` (with the Kubuno canvas), `on_paint_background`, `on_click`, the mouse, keyboard, focus,
    validation, layout and property-changed methods, `process_cmd_key`, `process_dialog_key`, `is_input_key`,
    `is_input_char`, `wnd_proc` (a message pre-filter), `get_preferred_size`, `set_bounds_core`, `create_params`,
    `on_create_control`, `dispose`, plus per family `on_checked_changed`, `on_selection_changed`, `on_value_changed`,
    `on_form_closing`… Rust code subscribes to a control's events directly (`button.click().subscribe(…)`).
  - WinForms `ControlStyles` (selectable, standard click/double-click, resize redraw, opaque…), `DesignMode`,
    `Invalidate`/`Refresh`, `Focus()`, `PerformClick()`, `SuspendLayout`/`ResumeLayout`.
  - **`ControlHost`**: controls built in Rust code, painted and driven outside any view with the same input routing
    as a view (mouse, keyboard, focus and validation sequences).
- The component registry records each element's class chain (`base_chain` in the `kubuno/registry` export, and
  `inherited_from` on the events an element inherits); the inherited events and default events now follow the chain,
  with the same results as before.

- **View lifecycle, threads and async for `.kbview` views (EVT-6 of vskubuno's `docs/EVENTS.md`)**, modelled
  on Windows Forms:
  - **FormClosing / FormClosed** on the view's root element (`OnFormClosing="..."`, `OnFormClosed="..."`): the
    window's close button, Alt+F4, the task bar, `host::close_window`, `Runtime::close` and the end of the
    Windows session raise FormClosing with its reason (`UserClosing`, `ApplicationExitCall`,
    `WindowsShutDown`...); a handler that sets `e.cancel` keeps the window open (unsaved changes), and the
    session end is refused with it. Otherwise FormClosed, then Deactivate, run before the window is destroyed.
    Rust code can also subscribe (`runtime.form_closing()`). The view root now also raises Move /
    LocationChanged when the window is moved.
  - **`UiDispatcher`** (`runtime.dispatcher::<MyViewModel>()`), the `Control.Invoke`/`BeginInvoke` of Kubuno
    views: any thread posts a closure that runs on the UI thread with `&mut` of the view model at the next
    frame, in posting order (`begin_invoke`, with an awaitable/waitable `AsyncResult`), or waits for its typed
    result (`invoke`); `invoke_required()`/`is_ui_thread()`. The window wakes up at once, even minimised.
    After the view closed, posted closures are dropped and waiting threads get `DispatchError::Closed` instead
    of hanging.
  - **Async event handlers**: an `async fn` in a `#[kubuno_views::event_handlers]` impl -
    `async fn refresh_click(ui: UiHandle<Self>, e: MouseEventArgs)` - runs on the UI thread and awaits without
    blocking it (`delay(...)`, any future); it changes the view model through `ui.update(|vm| ...)` and gets a
    copy of the event args. Setting `handled`/`cancel` from an async handler is refused at compile time.
    `spawn_local` starts other UI-thread tasks; closing the view cancels them all.
  - **`Timer`**, a non-visual component like Windows Forms' timer: `Timer::new("clock").with_interval(1000)
    .with_handler("clock_tick")`, added with `runtime.add_timer(&timer)`, ticks on the UI thread (late ticks are
    coalesced) and keeps ticking while the window is minimised.
- `kubuno_controls::host` gains what these need: `UiWaker` (wake a host window from any thread),
  `request_wake_after` (a timed frame that also runs when the window is minimised or hidden),
  `defer_close` / `InputEvent::CloseRequested` / `cancel_close` / `request_close` (a page that handles closing
  itself; a request nobody handles still closes the window). Nothing changes for applications that do not use
  them.

- **`kubuno-views-ls` handler commands (EVT-5 of vskubuno's `docs/EVENTS.md`)**, used by the Visual Studio
  designer: `kubuno/compatibleHandlers` lists the code-behind handlers an event can be bound to (typed methods
  whose sender and argument types fit the event, plus every `handlers!` table entry); `kubuno/renameHandler`
  renames a handler everywhere at once - every `On*="old"` of the views of the folder, the method (or the legacy
  `fn` and its `handlers!` string and forwarding call, or its `#[handler(name = "...")]`) and its `self.old(...)`
  calls -, or, after rust-analyzer renamed the Rust side, only the views and the strings; `kubuno/removeHandler`
  clears an event and also deletes its handler when it is still the untouched stub the designer created and
  nothing else uses it. F2 on a handler name in a `.kbview` renames it the same way.
- **"Handler not found" warnings in `.kbview` files**: an `On*` naming a handler the code-behind does not have, or
  one whose sender/argument types cannot take that event, is underlined, with quick fixes *Create handler `x`*
  and *Use `closest_name`*; an older event name (`OnToggled`) gets *Use `OnCheckedChanged`*. The warnings follow
  the code-behind when it is saved, without touching the view. Nothing is reported when the handlers are built
  at run time in a way the server cannot read.
- Every handler request (`createHandler`, `convertHandlers` and the new ones) accepts `openFiles`, the editors'
  unsaved texts, so its edits match what the editor holds.
- **Typed event handlers for `.kbview` views (EVT-4 of vskubuno's `docs/EVENTS.md`)**: a view's handlers can
  now be ordinary methods of its view model, in an `impl` marked `#[kubuno_views::event_handlers]`, like the
  handlers of a Windows Forms form - `fn on_hello_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs)`
  gets the concrete view model, the control that raised the event (with its current properties:
  `sender.text()`) and the event's own arguments (set `e.handled` or `e.cancel` through `&mut KeyEventArgs` or
  `&mut CancelEventArgs`). Shorter forms are accepted (`fn f(&mut self)`, `fn f(&mut self, e: &KeyEventArgs)`,
  `&dyn EventArgs` for any event, `&ElementRef` for any sender), `#[handler(name = "...")]` binds a method under
  another name and `#[handler(skip)]` leaves a helper out. A wrong signature is a compile error that says what
  is expected (async handlers, arguments by value, the sender after the args, a type that is not an event args
  type or a control...). Paint the view with `Runtime::frame_typed` (or `frame_typed_with`, which also takes a
  `handlers!` table for the handlers not converted yet). `use kubuno_views::prelude::*;` brings everything a
  code-behind uses, including one type per control (`kubuno_views::controls::Button`, `Switch`, ...).
- **`kubuno-views-ls` writes typed handlers**: in a code-behind with a `#[kubuno_views::event_handlers]` impl,
  creating a handler from the designer adds a typed method to it (the sender typed with the control, the args
  with the event's own type, `&mut` when it can be handled or canceled) and the prelude import if needed; a
  legacy code-behind keeps getting the legacy stub. A new code action on a `.kbview`, *Convert the handlers!
  table to typed handlers* (also the `kubuno/convertHandlers` request), rewrites a view's `handlers!` table into
  such methods mechanically and switches the `runtime.frame(...)` call to `frame_typed_with(...)`.

### Changed

- `kubuno_views::events::Component` (the trait a `Sender<C>` is typed with) is renamed `ElementType`: `Component` is
  now the root of the control hierarchy. `Sender<Button>` and existing code-behind files are unchanged.

- **Handler parameters are no longer reported as unused, and only them**: `#[kubuno_views::event_handlers]`
  exempts each handler's `sender`/`e` parameters (fixed by the event) instead of putting
  `#[allow(unused_variables)]` on the whole method, so an unused variable inside a handler's body is reported
  again. A syntax error inside the impl is now reported at its own location (the code is kept as written, so
  the rest of the file is still analysed), and the generated code no longer breaks when kubuno-views gains a
  field in its handler metadata (after updating the desktop sources, restart rust-analyzer so it reloads the
  rebuilt macro).
- `Runtime::frame_typed` / `frame_typed_with` require a `'static` view model type (every owned view model is).

- `kubuno/createHandler` with a name that is already a handler of the code-behind (picked from the Events tab's
  dropdown) binds the event to it instead of creating `name_2`.
- **Click and DoubleClick now carry `MouseEventArgs`** (the button, the click count, the position relative to
  the control and the modifier keys; no button and no click when a button is activated from the keyboard),
  like the `MouseEventArgs` Windows Forms passes as `EventArgs`. A legacy `handlers!` entry still receives
  `Value::Bool(true)` for a click: existing views and code-behinds keep working unchanged.

- **A diagnostics sink for GUI applications (`kubuno_controls::host::diagnostics`, re-exported as
  `kubuno_ui::diagnostics`)**, installed by the host when its window opens (opt out with
  `HostOptions::diagnostics = false` or `diagnostics::disable()`): a `tracing` subscriber and a `log` logger
  (level `debug` in Debug builds, `info` in Release, `KUBUNO_LOG` to change it), the redirection of
  `println!`/`eprintln!` when the process has no console, and a panic hook that records the message, location
  and backtrace and shows an error dialog. Everything goes to `OutputDebugString` while a debugger is attached,
  otherwise to the rotating log file. An application with its own window loop calls
  `diagnostics::install(&diagnostics::exe_name(), true)`.
- **A Kubuno crash window**: when the UI thread of a Kubuno application panics, a small window with the
  Kubuno caption and theme says "<App> ran into an unexpected error and has to close" (in French when
  Windows is), with the message and its location, a *Show details* expander with the backtrace, and *Open
  log*, *Copy* and *Close* buttons; then the application closes. It names the application by its window
  title (`diagnostics::set_display_name`). While it is shown the application's window keeps its look
  (Windows' "not responding" ghost window, with the default white caption, is turned off), and one crash
  shows one window (a panic can no longer produce a second report while unwinding out of the window
  procedure). A panic on a background thread is logged only.

- **`.kbview` controls now raise the Windows Forms input events, in the Windows Forms order (EVT-2 of
  vskubuno's `docs/EVENTS.md`)**: every control can handle `OnMouseDown`, `OnMouseUp`, `OnMouseMove`,
  `OnMouseEnter`, `OnMouseLeave`, `OnMouseHover`, `OnMouseWheel`, `OnClick`, `OnDoubleClick`, `OnMouseClick`,
  `OnMouseDoubleClick`, `OnKeyDown`, `OnKeyPress`, `OnKeyUp`, `OnEnter`, `OnGotFocus`, `OnLeave`,
  `OnLostFocus`, `OnValidating` (cancel it to keep the focus), `OnValidated`, `OnResize`, `OnSizeChanged`,
  `OnMove` and `OnLocationChanged`, and the view's root element `OnLoad`, `OnShown`, `OnActivated` and
  `OnDeactivate`. They arrive exactly as in Windows Forms: MouseDown, Click, MouseClick, MouseUp for a
  click; DoubleClick and MouseDoubleClick on the second click (a button gets a second Click instead); Enter,
  MouseMove, one MouseHover after 400 ms at rest, then MouseLeave; KeyDown, KeyPress, KeyUp to the focused
  control (Space or Enter on a button clicks it); the focus sequences for the keyboard and for the mouse,
  with Enter/Leave on the containers too; Load, Activated and Shown when the view first appears. The mouse
  goes to the innermost control under the pointer, and a pressed control keeps it until the button is
  released. Existing `handlers!` tables receive these events with the same value shape as before, and the
  events a control already raised (`OnClick`, `OnChanged`...) keep their values; a handler registered with
  the new `HandlerTable::insert_typed` receives the sender and the typed, writable arguments instead (mark a
  key handled, cancel a validation). The events of a frame are also returned as `ViewEventKind::Other`.
- **Event metadata in the `kubuno-views` registry (EVT-3)**: every event has a category (Action, Mouse, Key,
  Focus, Behavior, Layout, Property Changed...), an arguments type, cancelable/routing flags and the older
  names it still accepts, and every component a default event (`OnClick` for a button, `OnCheckedChanged`
  for a switch or a check box, `OnTextChanged` for the text fields...). Several events got their Windows
  Forms name: `OnToggled` on a `Switch` is now `OnCheckedChanged`, `OnChanged` on the text fields
  `OnTextChanged`, on `Dropdown`/`ComboBox` `OnSelectedValueChanged` and on `DatePicker` `OnValueChanged`,
  `OnActivate` on `ListView`/`TreeView` `OnItemActivate`. **The old names keep working** in every existing
  view; the language server shows a hint naming the new one. The registry export carries all of it for
  the Visual Studio designer, the language server completes and documents the new events, and
  `kubuno/createHandler` finds a handler written under an older name.
- **Double-click on a control in the `.kbview` design surface** asks the host to create or open that
  control's default event handler (new `doubleClick` surface message).
- **`kubuno_ui::FocusRing` records why the focus moved** (`take_changes()`: pointer, keyboard, program or
  removal, in order) and can put it back without a new move (`restore()`), which the event system above
  uses for the focus and validation sequences. `kubuno_ui.dll` changed: every application of the workspace
  was rebuilt against it.

- **Typed, Windows Forms-like event system for `kubuno-views` (core, EVT-1 of vskubuno's `docs/EVENTS.md`)**:
  new `kubuno_views::events` module - the `EventArgs` trait with `Handled` / `Cancelable` capabilities and a
  compile-time ancestor chain (`ArgsChain`, e.g. `FormClosingEventArgs` -> `CancelEventArgs` -> `EventArgs`),
  `#[derive(EventArgs)]` (new `kubuno-views-macros` crate; `#[args(handled, cancel, extends = ..., legacy = ...)]`),
  the standard args catalogue (mouse, key, key press, cancel, form closing/closed, drag and drop, scroll,
  layout, value/text/checked/selection changed with a change source, property changed, hot reloaded, and a
  paint placeholder), `Event<A>` multicast events whose `subscribe()` returns a `Subscription` that
  unsubscribes when dropped (subscription order, handlers added during a raise run from the next one,
  handlers removed during a raise are skipped, re-entrant handlers skipped instead of panicking, nesting
  capped at 32, `Handled` stops the raise), the read-only `ElementRef` / `Sender<C>` senders and a deferred
  `ControlQueue` for focus / select-all / property overrides. Purely additive: nothing raises these events
  yet, and existing `handlers!` tables keep working unchanged.

- **Multi-selection in `kubuno-views`' design surface (`view_embed`, vskubuno's `.kbview` designer), like
  the Windows Forms designer**: `design::Selection` (ordered ids + a primary), marquee (rubber-band)
  selection started on a container's empty area or on the canvas (`marquee_hits`: the children of that
  container the rectangle touches; Ctrl toggles, Shift adds, Esc cancels), Ctrl/Shift+click, Ctrl+A
  (`select_all_siblings`), group move with snaplines on the group's bounds, group resize from the primary's
  handles (`apply_edge_deltas`), multi-element nudge and Delete (one `editRequests` batch, new `"delete"`
  gesture), primary/secondary grab handles and the marquee painted by `paint_adorners`/`paint_marquee`.
  Drags now write whole DIP values.
- **Layout commands** (`design::FormatCommand`, `format_ops`): align, make same size, horizontal/vertical
  spacing (equal, increase, decrease, remove) and center in the container, computed from the painted
  layout on the top-level selected, non-docked Anchor children - requested by the host with the new
  `format {command}` message and answered as one `editRequests {gesture: "format"}`.
- **Protocol**: `selectionChanged` carries the whole selection (`ids`, the primary stays `id`); new
  host messages `selectMany {ids, primary}` and `format {command}`.
- **`kubuno-views-ls`**: new `reorderChildren {parentId, order}` operation (`edit::reorder_children` -
  Bring to Front / Send to Back of several siblings in one edit); `insertFragment` now accepts several
  elements (a multi-selection pasted or duplicated at once), each laid out on its own line with unique
  `x:Name`s.

- **Resizable design canvas in `kubuno-views`' `view_embed` (vskubuno's `.kbview` designer), like the
  Windows Forms designer**: the view is shown inside a Kubuno window frame (title bar with the root's
  `Title`) at its design size on a dark neutral canvas, with scrollbars when it is larger than the pane.
  Three handles (right edge, bottom edge, corner) resize it with a live relayout and a "width × height"
  tooltip; the release sends ONE batched edit writing the root's `Width`/`Height` when it has them, else
  the new design-time `DesignWidth`/`DesignHeight` (800×600 by default), plus the new place of every
  child its anchors moved (WinForms serializes them the same way). Clicking the canvas or the title bar
  selects the view itself.
- **Design-time attributes `DesignWidth`/`DesignHeight`** (`registry::DESIGN_TIME_ATTRIBUTES`): accepted
  on the root element only (the validator reports them elsewhere), ignored at runtime; offered by the
  language server's completion on the root only, with hover docs.
- **Design surface context menus and shortcuts**: a right-click (or Shift+F10 / the context-menu key)
  selects the element under the pointer - or the view outside its client area - and asks the host for
  its context menu (`contextMenu {x, y, screenX, screenY, elementId}`); Ctrl+C / Ctrl+X / Ctrl+V /
  Ctrl+D send `command {name, elementId}`.
- **New `kubuno/applyEdit` operations in `kubuno-views-ls`**: `insertFragment` (paste/duplicate: laid out
  on its own indented line, colliding `x:Name`s renamed `name2`, `name3`...), `wrapElement` (wrap an
  element in a new container, re-indented) and `unwrapElement` (replace a container by its children) -
  `kubuno_views::edit::{insert_fragment, wrap_element, unwrap_element, collect_names, unique_name}`.
- **Warnings (not errors) for `Dock`/`Anchor` set on an element whose parent is not a Dock/Anchor
  container** (`validate::warnings`, published by the language server with Warning severity); the view
  still compiles. `Dock="None"` is now accepted.


- **`kubuno-views`' `view_embed` (the design surface of vskubuno's `.kbview` designer) accepts drags
  from Visual Studio's Toolbox directly**: it registers its own OLE drop target (`mod ole_drop`), reads
  the component name from the `Kubuno.Views.ToolboxItem` clipboard format and feeds the existing DSG-9
  `dragEnter`/`dragOver`/`drop`/`dragLeave` handling, so a drop inserts the component where it lands
  (the "not allowed" cursor follows the drop target's validity). Found live that OLE never reaches a drop
  target registered by the hosting `devenv.exe` on the surface's parent window. Example-only
  `windows` features (dev-dependency); the library is unchanged.
- **User-facing documentation for every `.kbview` component, property and event**, in English and
  French: the registry's `doc` strings (shown by the editor's hover and by Visual Studio's Properties
  window) are now short sentences for app authors, e.g. "Text displayed on the button." instead of
  implementation notes (kept as code comments). The French text is exported as `doc_fr`
  (`registry::docs_fr`, a test keeps every member covered). The layout attributes and `x:Name` hovers of
  `kubuno-views-ls` are rewritten the same way.
- **Designer selection looks like WinForms**: the selected element's frame is drawn a few pixels outside
  it (it was invisible on a blue primary Button) with grab handles on every selected element - filled when
  the element can be resized there, hollow otherwise.

### Changed

- **The desktop applications (Kubuno shell, Drive, Documents, Chat) never open a console window**, in Debug
  builds too - like a Windows Forms application. Their logs, `println!` output and panics now go to Visual
  Studio's Output window when a debugger is attached, and otherwise to `%LOCALAPPDATA%\Kubuno\logs\<app>.log`
  (rotated at 1 MiB). A crash shows an error dialog instead of closing silently.

- **Moving an element within its container (designer reorder, Bring to Front / Send to Back) keeps one
  element per line**: its own leading whitespace moves with it (`edit::move_child`).

### Fixed

- **`handlers!` accepts the entry the designer writes**: a handler created from Visual Studio (double-click
  on a control, or the Events tab) is added as `"name" => |vm, value| name(vm, value),`, which the macro
  refused (it only took a `{ ... }` body), so the project no longer compiled. The body can now be any
  expression, and an empty `handlers! {}` no longer warns.

- **The `.kbview` designer's marquee (rubber-band) selection is now drawn with real DOTS, like the
  Windows Forms designer**, instead of the 4-DIP dashes it shared with the parent-container outline:
  `kubuno_views::design::dotted_outline` walks the marquee rectangle's perimeter in DEVICE pixels
  (not DIPs) so each dot is exactly one device pixel of ink and one device pixel of gap, pixel-aligned
  and crisp at any DPI (checked at 175%) — flat, round-free squares, not `stroke_arc`'s round dashes.
  The parent-container outline (`paint_adorners`) still uses `dashed_outline` unchanged.
- **`<Panel>` `Anchor` and `Dock` now behave like Windows Forms** at runtime and in the designer: an
  anchored edge keeps its distance to the panel's edge when the panel is resized (Left+Right stretches,
  Top+Bottom stretches, an unanchored axis keeps its size and recentres) - the anchoring reference was
  re-created at every frame, so anchors never moved anything; it is now the panel's `Width`/`Height`,
  the view's design size for the root panel, else the panel's first layout. Docked bands follow WinForms
  z-order (the first child in the document docks first, a `Dock="Fill"` written after the bands takes
  the remainder), and a docked or anchored child without `Width`/`Height` keeps its natural size instead
  of collapsing to zero.
- **`kubuno-controls`' host no longer drops quick clicks — for every window it hosts, not just the
  `.kbview` design surface.** A press released before the next frame was built (a touchpad tap, a
  synthetic click) was never seen as a press: `WM_PAINT` is only synthesized once the message
  queue is otherwise empty, so a fast `WM_*BUTTONDOWN` + `WM_*BUTTONUP` pair could both be
  processed before the frame that would have shown the button down ever ran, and a widget's usual
  `down && !prev_down` edge check across frames never fired. `kubuno_controls::host` now latches
  each button's press per frame (`consume_button_edge`, unit-tested): the frame immediately after a
  press always reports that button down at least once, even if it was already released again by
  then, and the frame after that carries the real (released) level — so the release edge a widget
  checks for still arrives too. Fixed for the left AND right buttons (a fast right-click could
  silently fail to open a context menu the same way); the host now also tracks the MIDDLE button
  (`Frame::middle_down`, previously not read from `WM_MBUTTONDOWN`/`WM_MBUTTONUP` at all) with the
  same latching, for any control that wants it. `kubuno-views`' `view_embed` (the surface
  `vskubuno`'s designer embeds) previously worked around this locally by latching its own
  `WM_LBUTTONDOWN`; that workaround is now redundant and has been removed — behaviour is
  unchanged, it just relies on the host's own guarantee instead. The spike's diagnostic chrome
  (probe line, Save/Menu demo buttons) is now only painted with `--debug-probe`, so the designer
  shows the view alone.

- **The sign-in page's title no longer overlaps the shell header's "☰ Kubuno" brand text.**
  `login_view` is the one page that draws no rail (it "owns the whole window because there is
  nothing to navigate to yet" — `window.rs`), but the shell's own header band is still painted on
  every page including sign-in; `login_view::draw` drew its own "Connexion à Kubuno" title at
  `y = 0..64`, the same band, instead of below `crate::header::HEIGHT` the way every other page
  positions its title relative to `crate::chrome::content_area`. Sign-in has no rail to get that
  offset from `content_area` (which also shifts the left edge by the rail's width), so it now
  applies `crate::header::HEIGHT` directly; the form card and every field/button below it shift
  down with it, since they all derive from the same `page()` rect.

- **`kubuno-chat` failed to build**: `src/chat/.cargo/config.toml` was malformed TOML (an
  unescaped `\k`/`\c` inside a plain string is not a valid escape sequence — Windows path
  backslashes need `\\` or a literal `'...'` string). It was also stale: `chat` builds as part of
  the Windows workspace and must share the workspace's build directory to load the same
  `kubuno_ui.dll` as every other app — exactly the reason `drive`'s own `.cargo/config.toml`
  (which `chat`'s comment claimed to mirror) deliberately sets none. `chat`'s config now matches
  `drive`'s: no target-dir, just the explanation of why not.

- `kubuno_views::design::DesignController::press` now selects AND arms a Move/Resize/Reorder
  drag in the SAME press (gated by a new `DRAG_THRESHOLD`, so a plain click still just selects) —
  found during DSG-9's own visual check: the previous two-press design (select, then a second
  press to arm) read as "dragging an unselected element does nothing but select it".
- `kubuno_views::protocol::parse_host_message` now strips a leading U+FEFF (byte-order mark)
  before parsing a stdin line, rather than silently treating it as an unrecognised line. Found
  during the DSG-9 visual check: some `.NET` `StreamWriter` configurations emit a UTF-8 BOM
  preamble on the very first write to a stream, which could otherwise cause the design surface to
  silently drop the first `setText`/`setDesignMode` a freshly-launched host ever sends it. The
  real fix is on the host side (`vskubuno`'s `RustDesignSurfaceHost.Protocol.cs` now writes to the
  process's stdin as raw UTF-8-without-BOM bytes); this is the defence-in-depth half.

### Added

- **Move/resize drag, Flow reorder and toolbox drop in `.kbview` design mode** (`kubuno-views`,
  work package DSG-9 of `vskubuno/docs/DESIGNER.md` §10): `kubuno_views::design::DesignController`
  gained a drag state machine driven by real mouse input on `examples/view_embed.rs`'s own
  surface — pressing a resize handle or an Anchor element's own body arms a live-preview
  move/resize (snaplines against sibling edges/centres and the container's own bounds, Shift
  suppresses snapping, Esc cancels with no text write), pressing a Flow (`<Stack>`) child arms a
  reorder with a live insertion marker. Nothing is written to `.kbview` text until mouse-up: a
  move/resize emits every changed `X`/`Y`/`Width`/`Height` as ONE batched `editRequests` protocol
  message (`{ops, gesture:"move"|"resize"}`, a new addition to `kubuno_views::protocol`) so the
  host applies it as a single undo unit; a reorder emits one `moveElement` op. A new
  `kubuno_views::design::ToolboxController` answers the surface's own protocol additions
  (`dragEnter`/`dragOver`/`drop`/`dragLeave`, the host's translation of an OLE/WPF drag from the
  VS Toolbox) with live drop-target feedback (`dropTargetChanged`, incl. whether the current
  position is actually allowed — validated against the registry's `ChildrenModel`, mirroring
  `crate::validate`'s own gating rule) and, on a valid drop, an `insertChild` edit with a
  `<Component/>`/`<Component X="…" Y="…" Width="80" Height="24"/>` skeleton. 50 new/updated tests
  in `design.rs`, plus new wire-shape tests in `protocol.rs`; `cargo clippy --all-targets -- -D
  warnings` clean.

- **Design mode for `.kbview` views** (`kubuno-views`, work package DSG-6 of `vskubuno/docs/
  DESIGNER.md`): a new `kubuno_views::design` module records a per-frame **layout map** (every
  compiled element's stable id + painted bounds + parent id + parent's layout kind — `Anchor`/
  `Dock`, `Flow`, or none), additive and zero-cost when unused (`PaintCx::design` is `None` for
  an ordinary frame; `Runtime::frame` is unchanged, a new `Runtime::frame_with_design` opts in).
  `LayoutMap::hit_test` finds the deepest element under a point. `examples/view_embed.rs` (the
  exe `vskubuno`'s `RustDesignSurfaceHost` embeds) gained an actual design mode: while on, real
  mouse/keyboard input no longer reaches the compiled view's own widgets — a click selects the
  element under the cursor instead, Esc selects its parent, Delete/an arrow (nudge, Anchor
  elements only, Shift for a larger step) become edit requests rather than being applied
  locally, and selection/hover/parent-container adorners are drawn on top (8 resize handles on
  an Anchor/absolute element, a plain outline on a flow child). A new line-delimited JSON
  protocol (`kubuno_views::protocol`) on the surface's own stdin/stdout — documented as
  `vskubuno/docs/DESIGNER.md` §9 — carries `setText`/`setDesignMode`/`select` in and
  `selectionChanged`/`editRequest` out, replacing the temp-file bridge `RustDesignSurfaceHost`
  used until now; `view_embed`'s `<file.kbview>` CLI argument is now optional. `examples/
  view_preview.rs` is untouched.

- **Embeddable control host** (`kubuno-controls`): a Kubuno surface can now run as a child
  window inside another application's window, even one owned by a different process (the
  groundwork for the Visual Studio `.kbview` designer). Set `HostOptions::parent` to the
  parent window handle: the surface fills the parent's client area, follows the size the
  parent gives it, takes the keyboard focus when clicked, closes its open menus when the
  focus leaves it, follows the parent across DPI changes, and its process exits by itself
  when the parent window goes away, including when the parent process crashes. Existing
  top-level windows are unchanged. A new example, `kubuno-views`'
  `view_embed --parent <hwnd> <file.kbview>`, shows a `.kbview` embedded this way.
  - **Keyboard protocol for the embedded case** (additive, embedded mode only - a top-level host
    is unaffected): a key this frame's page did not consume is now re-posted to the parent as the
    same `WM_KEYDOWN`/`WM_SYSKEYDOWN` a real keystroke would have produced, so an embedding host
    (Visual Studio's own accelerator table, via `vskubuno`'s `RustDesignSurfaceHost`) can route
    Ctrl+S, F5, Ctrl+Z, Ctrl+Shift+B and the rest without this crate knowing anything about VS
    commands. A new `host::notify_tab_out(backward)` lets a page report that its own Tab/Shift+Tab
    handling ran past its last/first focusable control, posting a new `host::WM_KUBUNO_TAB_OUT`
    message the embedding host can turn into `MoveFocus`. `view_embed.rs` now has a small two-control
    demo focus ring (`save_btn`/`menu_btn`) exercising both ends of this protocol.
  - The forwarded key alone was not enough for an embedding host to correctly re-fire a modifier
    chord (Ctrl+S and similar): a Win32 `WM_KEYDOWN` carries no modifier state at all, and by the time
    the embedding host finally processes the forwarded message, the physical Ctrl/Shift/Alt keys may
    already be released again. `forward_unhandled_keys` now posts a new `host::WM_KUBUNO_KEY_MODS`
    message (the modifiers actually held at the key's original, physical moment) immediately before
    each forwarded key - same source thread, same destination, so delivery order is guaranteed - for
    the embedding host to use instead of reading its own, possibly-stale keyboard state.

- **Language support for `.kbview` view files** (`kubuno-views-ls`): a language server that
  editors such as Visual Studio use to check views as you type (parse and validation errors),
  complete element, attribute and value names, show the documentation of a component or
  property on hover, list the view's element tree, and jump from an event attribute to its Rust
  handler. It never reformats a view on its own.

- **`kubuno-views-ls`: editing support for the future visual designer.** Three new JSON-RPC
  methods let a client (the Visual Studio designer, or any other tool) make surgical edits to an
  open `.kbview` document and keep a design surface's selection in sync with the XML text,
  without ever regenerating the file: `kubuno/applyEdit` (set/remove an attribute, insert a
  child, remove an element, move an element — including into a different container — and
  rename an element, each returning the minimal `{range, newText}` edits to apply, computed
  against the document's current text), `kubuno/elementAtOffset` and `kubuno/rangeOfElement`
  (selection sync between the XML text and a design surface). Every element is addressed by a
  new **stable id** independent of `x:Name` (a dot-separated path of child-ordinal indices from
  the document root). `kubuno-views`' surgical edit API (`edit.rs`) now returns the precise
  byte ranges it touches instead of a whole new document string, and gained a cross-container
  `move_element` and a `rename_element`. See `vskubuno/docs/DESIGNER.md`'s "DSG-2 protocol"
  section for the full wire format.

- **`kubuno-views-ls`: `kubuno/registry`, the component registry as JSON.** A
  new JSON-RPC method returns the whole component registry (name, doc,
  toolbox family, an icon hint, the children model including any gated
  allowed-child names, the layout engine kind, and every property/event) so
  the future Visual Studio designer's toolbox and property grid can be built
  from it instead of a duplicated, hand-maintained list. The response also
  carries a content-derived `version` the client can use to cache the
  result. New `kubuno-views::registry::export` module does the JSON
  conversion; see `vskubuno/docs/DESIGNER.md` §5/§8 for the wire shape and
  the handful of points where it deliberately follows the already-written VS
  consumer over that design note's own first sketch (field-name casing, and
  `LayoutKind`'s real variant set).

- **`kubuno-views-ls`: `kubuno/createHandler`, double-click a control/event to create its
  handler.** A new JSON-RPC method (work package DSG-10, `vskubuno/docs/DESIGNER.md` §6/§8)
  extends `definition.rs`'s existing "event attribute → `fn` in a sibling `.rs` file" lookup with
  its write half: given an element id, an event name (`OnClick`, `OnToggled`…) and an optional
  suggested name, it picks a unique `on_<xname or element>_<event>` handler name, sets the
  `On*="…"` attribute on the `.kbview` element, and inserts both a real `fn <name>(vm: &mut dyn
  ViewModel, value: Value)` stub into the code-behind `.rs` file (found the same way
  `definition.rs` finds handlers) and, when that file already has a `handlers!` table, a
  forwarding registration entry — a real `fn` is always created (never only a closure inside the
  table) so "go to definition" keeps working. Every insertion is a pure, zero-length text
  splice — nothing existing is ever regenerated or reformatted. If the event already names a
  handler, the response is just that handler's existing location instead of a new edit. The
  result travels as a standard `lsp_types::WorkspaceEdit` (`{changes: {uri: [TextEdit]}}`)
  covering both files.

- **`kubuno-views` crate: the foundation of declarative XML views.** A new
  crate, `src/crates/kubuno-views`, lays the first-phase groundwork for
  writing Kubuno desktop screens as `.kbview` XML files instead of hand-rolled
  Rust layout code (see `vskubuno/docs/XML_VIEWS.md` for the full design):
  - a **lossless XML syntax tree** (built on `rowan`, the same foundation as
    rust-analyzer and the TOML tooling behind Even Better TOML): parsing any
    file, even a broken one, always reconstructs it byte-for-byte, and every
    parse error carries a line and column;
  - a **surgical edit API** (set/insert/remove an attribute, insert/remove/
    move a child element) that changes exactly the bytes it needs to and
    leaves comments, formatting and attribute order untouched everywhere
    else — the foundation the future visual designer and Claude will both
    edit `.kbview` files through;
  - a **component metadata registry** describing five real desktop
    components (`Button`, `Switch`, `TextField`, `Card`, `Stack`) — their
    properties, types, allowed values and children — read directly from
    `kubuno-ui`'s existing API, with nothing in `kubuno-ui` itself changed;
  - a **validator** that checks a parsed view against that registry and
    reports unknown elements/attributes, invalid values and misplaced
    children, each with a precise line and column — the same diagnostics
    engine a future in-editor preview and language server will reuse.

  This phase does not yet build or display anything from a `.kbview` file —
  that is the next step. Nothing about how existing desktop screens work
  today has changed.
- **`kubuno-views` crate: the XML views interpreter.** A `.kbview` file now
  actually renders, with hot reload and no restart:
  - `kubuno-views` gained an **interpreter**: a parsed and validated view is
    built into a live tree of the real `Button`/`Switch`/`TextField`/`Card`/
    `Stack` widgets, laid out with the same `kubuno_controls` engines
    hand-written screens use, and painted every frame with real mouse/focus
    interaction (hover, press, click, Tab order) — not a static picture;
  - **one-way and two-way data binding** (`Text="{Binding Path}"`,
    `On="{Binding Path, Mode=TwoWay}"`) against a small view-model interface,
    and named event handlers (`OnClick="save_clicked"`) dispatched from a
    handler table the code-behind builds with the new `handlers!` macro;
  - **hot reload**: editing and saving a `.kbview` file updates the running
    view without restarting the app and without losing the view model's
    state (what is typed into a field, what is toggled); a file that fails
    to parse or validate keeps showing the last version that worked, with an
    on-screen banner naming the file, line and column of the problem;
  - a new example, `cargo run -p kubuno-views --example view_preview -- <file.
    kbview>`, opens any `.kbview` file in a live, hot-reloading preview
    window — the fastest way to see a view while editing it, ahead of the
    Visual Studio designer.

  The component registry itself is unchanged (still the same five
  components); nothing in `kubuno-ui` was modified to support any of this.
- **`kubuno-views`: every component family is now integrated and on by default.**
  The five families written in parallel behind their own Cargo feature are now
  all enabled (`default = ["all-features"]`); `kubuno-views-ls`'s completion
  and hover, being registry-driven, list every one of them too:
  - **`kubuno-views`: the `display` component family (Label, LinkLabel, Badge,
    Icon, Separator, Spinner, ProgressBar, Callout, EmptyState) — live widgets
    with click handling on LinkLabel/Callout/EmptyState and animation on
    Spinner/ProgressBar.**
  - **XML views: `choice` family — IconButton, CheckBox, RadioButton, Slider,
    NumericField, with live interaction and two-way bindings.**
  - **XML views: `text` family — TextArea, SearchField, MaskedField, Dropdown,
    ComboBox, DatePicker, ColorField and the `Option` child element, with real
    floating panels.** Known gap: ColorField's picker panel isn't wired.
  - **XML views: `containers` family — Panel (Dock/Anchor), GroupBox,
    ScrollArea, Splitter, Tabs/TabItem, Breadcrumb, Toolbar, Accordion,
    Stepper.**
  - **XML views: `data` family — ListBox, CheckedListBox, ListView, TreeView,
    DataTable, MonthCalendar with bindable selection, events and wheel
    scrolling.**

  A single shared icon-name → glyph table (`kubuno-views::icon`) replaces the
  three the `display`/`choice` families and the phase-2a `Button` used to
  each hand-roll: a short lower-case alias list, falling through to the real
  embedded Lucide icon set (`drive_app_controls::icon_name`, the same lookup
  `kubuno_ui::navigation`/`editors`/`lists` already use) instead of a
  cosmetic, possibly-invalid name. `crate::node::PaintCx::fire`/
  `InteractCx::fire`/`press_release` are now `pub(crate)` and shared by every
  family, removing each one's own copy of that dispatch.

- **XML views: typed children.** `ChildrenModel::List` now carries the
  allowed child element names (empty for an ordinary widget container like
  `<Stack>`/`<Panel>`/`<Splitter>`). `crate::validate` uses this, registry-wide,
  to reject a "gated" child under the wrong parent wherever it is nested —
  `<TabItem>` outside `<Tabs>`, and likewise `<Item>`/`<Column>`/`<Option>`/
  `<Step>`/`<AccordionSection>`/`<BreadcrumbItem>`/`<ToolbarItem>` outside
  their own parent — with a line/column diagnostic naming the valid parent(s).
  `ComponentMeta` also gained a `LayoutKind` (`None`/`Flow`/`DockAnchor`/
  `Split`/`Tabs`) describing which layout engine a container drives, metadata
  only, for the future visual designer's drop visualization
  (`vskubuno/docs/DESIGNER.md` §4/§6 DSG-1).

- **XML views: bindable lists (`ItemsSource`).** `crate::binding::Value`
  gained a `List(Vec<Row>)` variant (`Row` = named fields, in order) — what
  `ItemsSource="{Binding Path}"` now actually reads, every frame, on
  `ListBox`/`CheckedListBox`/`ListView`/`TreeView`/`DataTable` (each row's
  `Text` field, or — for `ListView`/`DataTable` — one cell per `<Column
  Binding="{Binding Field}">`) and on `Dropdown`/`ComboBox` (`DisplayMember`/
  `ValueMember`, defaulting to `Label`/`Value`, name which row field is the
  option's label/value). Static `<Item>`/`<Option>` children remain fully
  supported and are what is used whenever `ItemsSource` names no binding, or
  the binding is unset or not currently a list. Known gaps: a bound
  `CheckedListBox` row always starts unchecked (no check-state field), and a
  bound `TreeView` is always one level deep (`Value::List` is flat).

- **XML views: `Splitter` drag.** The divider now actually drags — mouse
  (press-and-hold on the grab band follows the pointer, `Splitter::drag_to`'s
  own contract) and keyboard (once the bar is focused: the two arrows along
  the split axis, Shift for a larger step, Home/End, Enter to collapse/
  restore — `Splitter::handle_key`'s own WAI-ARIA "window splitter"
  behaviour) — writing `Distance` back when bound `Mode=TwoWay` and
  dispatching the new `OnDistanceChanged` event.

- **Office ribbon component.** The ribbon used by the web Office editors is now a
  shared desktop component (`kubuno_ui::ribbon`), declared as data like on the
  web (tabs → groups → items):
  - **tab strip**: coloured in the app's tone (Documents, Spreadsheet,
    Presentation…) or plain with an accent underline. It has the « Fichier »
    tab, quick actions (Save, Undo, Redo), and contextual tabs with a coloured
    rule that the ribbon switches to when they appear;
  - **groups**: labels and rules, with small buttons stacked three to a column;
  - **items**: large buttons, toggles, font and size drop-downs, split buttons
    and menu buttons with their menus, galleries, and custom slots;
  - **tooltips** show the label and shortcut.

  When space runs out, the right-most groups fold into buttons that open the
  whole group in a floating panel. The ribbon collapses with Ctrl+F1 or its
  chevron, and a tab click then shows it as a floating flyout. The « Fichier »
  tab opens a Backstage (accent rail of sections). It follows the light and
  dark themes. The gallery has a new « ribbon » page with three live ribbons.
  Icons the ribbon needs (copy, cut, paste, format painter, save, undo/redo,
  print, export…) were added to the shared icon set.
- **Tab panels scroll their content by default.** When anything painted in a
  tab's page reaches past the panel's edges, a vertical and/or horizontal scroll
  bar appears (Kubuno's thin resting indicator, unfolding with its arrows under
  the pointer). The page scrolls with the wheel (Shift+wheel sideways), a thumb
  drag or a click on the track, and each tab keeps its own position. The page
  does not have to declare its size: the panel measures what is drawn, text
  overflowing its box included. Lists, sliders and other controls that use the
  wheel themselves keep it, so the page does not scroll along with them. Hover,
  clicks, menus and keyboard focus keep working inside scrolled content. The
  new `ScrollArea` component provides the same behaviour to any region.
- **Keyboard, wheel and focus for the desktop components.** The desktop host now
  forwards key presses and typed text (emoji included), the mouse wheel,
  double and triple clicks, and keeps the mouse while a button is dragged
  outside the window; a page can set the pointer cursor, read and write the
  clipboard, and ask for a timed repaint (caret blink, animations). A shared
  focus manager gives every component the web's behaviour: Tab and Shift+Tab
  walk the controls, a click focuses, and the focus ring shows only after
  keyboard use (`:focus-visible`). An input method's in-progress composition
  is available too (`InputEvent::Composition`, `host::composition()`), so an
  editor can draw it inline. In the component gallery, Ctrl+Tab and
  Ctrl+PageUp/PageDown switch pages.

- **A rich text editor.** `richtext::RichTextBox` is the editing area the rich
  text toolbar was waiting for: paragraphs, headings, bulleted and numbered
  lists, bold, italic, underline, strike-through, links and inline code, with
  word wrapping, a caret and selection by mouse and keyboard, the clipboard,
  undo and redo, and scrolling. The gallery's new « richtext » page drives it
  from the existing toolbar.

- **A composition page in the component gallery.** Six realistic screens —
  settings, a data table, a file explorer, feedback states, a form dialog and
  an overflow audit — built from the components nested several levels deep,
  plus a live form in a scrolled container whose drop-downs, menus, help bubble
  and tooltip escape the container. It is the regression page for everything
  that only goes wrong once components are combined.

- **The help bubble from the web design system.** `help::HelpBubble` is the
  accent-coloured bubble the web console opens next to a « ? »: a bold title,
  a paragraph, an « OK » button and an optional second action, with an arrow
  pointing at the control it explains. It goes on whichever side has room —
  below first, then above, right, left — stays inside the window, and keeps its
  arrow on the anchor even when pushed back from an edge. `help::HelpButton` is
  the « ? » itself (quiet at rest, accent while its bubble is open). The
  component gallery shows both on the fields page, with a live « ? » whose
  bubble closes on « OK » or on a click anywhere else, as on the web.

- **Tooltips, menus and help bubbles can reach beyond their window.** Each is
  drawn in a transparent top-level window of its own, owned by the window that
  opened it, so it sits above everything and can hang past that window's edges,
  kept inside the monitor rather than the window — as native menus do. Tooltips
  let the pointer through (`host::overlay`); menus, context menus and help
  bubbles take it (`host::popup`): hovering and clicking them works even where
  they lie outside the window, without taking the keyboard focus from it.
  Several can be open at once (a menu and its submenu). A click on the desktop
  or in another application closes them, as on the web. The host now also
  reports the right mouse button, so a real right-click context menu is
  possible — the component gallery shows one on the lists page, and its help
  bubble on the fields page.

- **Every gallery page now carries a live column.** To the right of each page's
  static exposition, an interactive column shows the same controls driven by the
  real pointer, so a control can be tried, not just read: buttons whose counter
  climbs, switches/checkboxes/radios/segmented toggles that flip, list rows and
  tree nodes that select and expand, tabs that switch, dropdowns and menus that
  open, sliders and scrollbars and colour pickers you drag, a calendar whose day
  you pick, dialogs that open and dismiss, an accordion and a stepper that move,
  a data table you sort, select and page. Each control remembers its state
  between frames. Built on one shared helper (`pages/interact.rs`) so every page
  works the same way, with the mouse and — since the host forwards it — the
  keyboard.

- **The Kubuno UI gallery is now one navigable window.** It opened one page per
  launch before; a strip of tabs across the top now switches between every
  component family — buttons, text, fields, lists, range, containers,
  navigation, display, views, colour, date/time, dialogs, editors, feedback and
  tables — so the whole design system is explorable from a single `gallery`
  example. `--page <name>` still opens straight to a page.

- **kubuno-ui gained the outlined form field.** `fields::OutlinedField` is the
  Material outlined text field the web design system builds every form field on:
  the label rests inside the box as the hint and floats up onto the border
  (opening a notch) once the field is focused or holds a value, focus paints the
  border and label in the primary colour, and it supports a leading icon, a
  trailing glyph, a required asterisk, a large variant, multiline, an invalid
  state and read-only. It is the base the Contacts-style composites
  (address, phone, date, labelled groups) will be built on — the one part of the
  web `@ui` library the desktop port had not yet covered.

### Changed

- **The desktop shell is built from the shared Kubuno components.** The
  launcher and its administration console now use the design system's own
  components instead of hand-drawn copies:
  - **navigation**: the side rail and its administration tree, and
    breadcrumbs in every admin page;
  - **header and panels**: round header buttons, the storage gauge, and scroll
    bars in the apps panel and the account panel (which had none);
  - **settings controls**: switches, the theme choice (radio buttons) and the
    sync interval (number field with steppers);
  - **cards and badges**: the Home, Settings, Accounts, Activity, Storage,
    Dashboard and Settings-admin cards, role/status/billing badges and
    separators;
  - **tables**: the Users, Modules and Audiences tables, with their search
    boxes;
  - **states and messages**: empty, loading and error states (the login
    error included).

  The look now matches the other Kubuno apps exactly (rail rows,
  hover colours, gauge thresholds at 75 %/90 % like the web). On the Home
  page the connection switch now shows ON when online, matching its label.
- **The desktop shell runs on the shared Kubuno window.** Its window,
  title bar, keyboard and mouse handling are now the ones every Kubuno
  desktop app shares. It keeps its own header (with the Mica backdrop),
  closes to the notification area, and follows theme and font changes live.
  Resizing now works from every edge, not only the top.
- **Real text fields in the shell.** The sign-in fields (password masked),
  the proxy field and the new-label field are now the design system's text
  fields: selection, copy/paste, undo, word navigation, a right-click menu,
  a blinking caret, and Tab / Shift+Tab between fields.
- **Confirmations inside the app.** Deleting a label now asks in a Kubuno
  dialog (« Supprimer » / « Annuler », Enter / Escape) instead of a Windows
  message box, and disconnecting an account or signing out now asks for
  confirmation first.
- **Escape closes the open apps or account panel**, and a click outside the
  application closes it too.
- **The title bar matches the ribbon.** In a desktop app with a ribbon, the
  window's title bar (and its border) now takes the colour of the ribbon's tab
  strip — the app's tone, e.g. the Documents blue — so the two read as one
  band, as on the web. The colour changes with the ribbon, and apps without a
  ribbon keep the usual Kubuno blue.
- **Documents uses the shared Kubuno ribbon and window.** The desktop word
  processor now wears the same ribbon as the web editor: the same tabs
  (Fichier, Accueil, Insertion, Mise en page, Références, Affichage, Révision),
  groups and commands, in the Documents blue. It has quick actions,
  drop-downs with the machine's real fonts, menus, folding groups, collapse
  (Ctrl+F1) and a Backstage (Informations, Fermer). Its window now uses the
  common Kubuno window frame, so menus and lists can extend beyond it like in
  the other apps. The zoom commands (100 %, one page, page width) and the
  ruler toggle work; commands that need document editing are listed but do
  nothing yet, as before. Quick actions stay disabled until saving and undo
  exist. The status bar takes the web's light grey.
- **The desktop apps share one component library, `kubuno_ui.dll`.** The Kubuno
  design system — every component, the window host and the painting surface —
  is now built once as a shared library that the shell, Drive, Chat, Documents
  and the component gallery all load, instead of each app carrying its own copy.
  The component gallery therefore exercises exactly the code the apps run. Drive,
  which used to be built on its own, is now part of the same build so that it can
  share the library. Builds link Rust's standard library dynamically too: after
  a build, `tools/stage-runtime.ps1` places `kubuno_ui.dll` and Rust's `std` DLL
  next to the programs, and the MSIX package ships both.

- **Every desktop component family is now closer to its web counterpart.**
  Sizes, spacing, colours and states were re-read from the web design system,
  and each family now works from the keyboard as well as the mouse:
  - *Buttons, check boxes, radio buttons, switches*: transparent grounds,
    labels that wrap or end in an ellipsis instead of being clipped, the
    keyboard focus ring, a loading state, descriptions and smooth toggles.
  - *Text fields, form fields*: fully editable — caret, selection by mouse and
    keyboard, clipboard, undo/redo, maximum length, horizontal scrolling and
    wrapping, a right-click edit menu; the outlined field's floating label no
    longer gets cut off by its container; search fields match the other
    fields' height.
  - *Lists, combo boxes, menus*: full keyboard navigation with type-ahead and
    wheel scrolling; combo lists and menus open in floating windows that stay
    on screen and flip when needed; submenus open to the left at the screen
    edge.
  - *Sliders, scroll bars, number fields*: keyboard, wheel and drag; number
    fields can be typed into; the slider's value bubble floats while dragging.
  - *Cards, panels, group boxes, scroll views, splitters*: content picks up its
    container's background (no light squares in the dark theme), stays inside
    rounded borders, and scrolls with the wheel and keyboard; group box
    captions sit in their frame and end in an ellipsis only when too long.
  - *Tabs, toolbars, sidebar, breadcrumb, status bar*: overflowing tab strips
    scroll and keep the selection in view; overflow menus float; everything is
    keyboard navigable.
  - *Labels, links, badges, tooltips*: text never paints outside its box
    (ellipsis, clip or wrap), badges can be capped, tooltips wrap at the web's
    width and appear after the web's delay.
  - *Tree and list views*: the web explorer's selection, full keyboard
    navigation, multi-selection, inline rename and an overlay scroll bar.
  - *Colour, date and time pickers*: their panels float in their own windows;
    full keyboard support; dates can be edited segment by segment.
  - *Dialogs, popovers, toasts*: long words wrap, Tab stays inside the dialog,
    Escape closes and Enter confirms, the prompt field is editable, popovers
    float, toasts stack, expire and pause on hover.
  - *Editors (drop-down, font picker, font size, inline rename)*: floating
    lists, typing in the font search, real inline editing.
  - *Feedback (empty state, callout, accordion, stepper, spinner)*: text wraps
    without losing words, keyboard navigation, a compact stepper in narrow
    containers.
  - *Data table*: the bulk-action bar folds actions into its menu instead of
    cutting the selection count; custom cell painters.

- **The desktop design system now renders text at the native OS form size.**
  The default body text moved from the web's 14 px to Segoe UI 9 pt (12 px), the
  size Windows forms use — so a native desktop application reads at the
  platform's size rather than a browser's. It is one shared constant, so every
  component and every desktop app (shell, chat, the documents chrome) shifts
  together, and the controls keep their height: only the glyph shrank, giving
  labels a little more room in the same box. Headings and titles are unchanged.

### Fixed

- **`kubuno-views-ls` now works inside Visual Studio.** It advertised the bare
  `textDocumentSync: 1` shorthand, which Visual Studio's LSP client reads as "no open/close
  notifications", so it never sent `didOpen`/`didChange` and no diagnostics ever showed up; it now
  advertises `{ openClose: true, change: Full }` explicitly. It also never exited after the client's
  `exit` or when the client closed its stdin (it joined its I/O threads while still holding the
  connection, a deadlock), leaving an orphan process behind every Visual Studio session; it now exits
  cleanly in both cases.
- **XML views: `<MonthCalendar>`/`<DatePicker>` show the real « today »,
  not `01/01/2000`.** Neither node ever called `set_today_date`, so both
  stayed at `kubuno_controls::datetime::DEFAULT_TODAY` — the replica's
  deliberately implausible sentinel for "nobody set a real one" — forever.
  A new `crate::clock::today()` (`GetLocalTime`, the local calendar date,
  not UTC's) now seeds it every frame on both. Each also gained an optional
  `Today` attribute (ISO `YYYY-MM-DD`) that overrides it when set, for a
  test or a screenshot that needs a fixed date instead of whatever day it
  happens to be run on — the same reason the `kubuno-ui` gallery's own
  demo pages hard-code a `TODAY` constant, now available per-view instead
  of only by editing Rust.
- **XML views: a nested scrollable data control no longer swallows wheel
  notches it does not need.** `ListBox`/`CheckedListBox`/`ListView`/
  `TreeView`/`DataTable` used to `claim_wheel()` on any wheel travel over
  their bounds, even a short list with every row already on screen — so
  hovering one inside a `<ScrollArea>` (a `<Data>` tab with a three-row
  `<ListBox>` above a `<MonthCalendar>`, say) blocked the page from ever
  scrolling past it. Each one now claims the wheel only after checking that
  its OWN scroll position actually changed (standard scroll chaining: an
  already-at-its-limit or nothing-to-scroll control lets the wheel fall
  through to whatever hosts it), through one shared, unit-tested
  `registry::families::data::claim_wheel_if_scrolled` helper instead of each
  repeating (and, for three of the five, getting slightly wrong) the same
  check. `TreeView`'s own scroll also gained an upper clamp it never had
  (`kubuno_ui::views::TreeView` has no `max_scroll` of its own, derived here
  from its `rows()`/`row_height()`) — without one, scrolling down always
  "moved" even past the last row, which would have kept it claiming the
  wheel forever too. `MonthCalendar` is unchanged by design: its wheel
  changes the shown month, not a content offset with a limit, so it keeps
  claiming unconditionally, like a spin control would.
- **XML views: a `<Stack>` column no longer stretches or mis-sizes its
  children.** `ViewNode` gained two default (opt-in) methods that
  `registry::families::containers::StackNode`/`crate::node::StackNode`
  (the `<Stack>` interpreter) now use:
  - `measure_for_width` — a leaf whose height depends on the width it is
    actually given (`<Callout>`, whose `kubuno_ui::feedback::Callout::measure`
    is `w-full` and otherwise wraps its body into a designer placeholder
    width) now reports its real height against the `<Stack>` column's real
    row width instead. Fixes a `<Callout>` sometimes claiming a wildly wrong
    (too tall) block and pushing every sibling after it out of view (an
    `<EmptyState>` after a `<Callout>` in the same column, say).
  - `intrinsic_width` — a fixed-content-size leaf (`<Badge>`, `<IconButton>`,
    `<SearchField>`, `<ColorField>`: each paints across whatever bounds it is
    given, by design, rather than limiting itself) is now narrowed to its own
    measured width and left-aligned inside its column's full-width block,
    instead of stretching edge to edge (a `<Badge>` reading as a full-width
    pill) or painting centred somewhere off to the right (an `<IconButton>`).
    A `w-full` leaf (`<Separator>`, `<ProgressBar>`) is unaffected — its
    default `None` answer keeps the existing full-width behaviour.
- **XML views: a `<Stack>` child can now pin its own `Height`/`Width`.** A
  literal (non-`{Binding …}`) `Height` (in a vertical `<Stack>`) or `Width`
  (horizontal) attribute on a direct `<Stack>` child now overrides that
  block's measured extent — the `Height`/`Width` attributes were already
  accepted everywhere by `crate::validate` but, outside a `<Panel>`'s
  Dock/Anchor children, silently did nothing. Needed by `<Splitter>` (its own
  measured size is a designer placeholder unrelated to its two panes'
  content) and useful for any other child whose natural measurement is not
  the size a view actually wants.
- **XML views: `<ListView>` static `<Item>` rows now carry every column, not
  just the first.** `<Item Text="Alice" Role="Admin"/>` now fills the
  primary column from `Text` and any further declared `<Column Binding=
  "{Binding Role}">` from an attribute named exactly like that column's bare
  binding field (`Role`) — previously only `Text` was ever read, so every
  column past the first was blank for a static (non-`ItemsSource`) row.
  `<Item>` is now the one component whose attributes are not a fixed, closed
  list (`ComponentMeta::open_attributes`) — its real fields are named by
  whichever `<Column>`s its parent declares, which no static `properties`
  table could enumerate.
- **No more "entry point not found" after a partial build.** Rebuilding one
  desktop app used to leave the others linked against the previous shared
  component library, and they then refused to start. A new
  `tools/build-all.ps1` builds every app together, and `stage-runtime.ps1`
  now names any app that is out of date with the library, with the command
  that fixes it.
- **Admin « Unités organisationnelles » shows its loading animation.** The
  page's loading state was never recognised (a mismatched section id), so it
  stayed still while the data loaded.
- **The last quick link on the Home page no longer overflows its card** by
  16 DIP on the right.
- **Font lists no longer stay empty.** An app that declared its font picker
  before its window opened got an empty list of fonts for the whole session;
  the fonts are now read again once the window is up. A drop-down whose
  value is not among its choices (a font the machine lacks) now shows that
  value instead of an empty field.
- **Scroll bars stay inside rounded components.** In a component with rounded
  corners (list box, drop-down list, font and size pickers, multi-line text
  field, rich text box, data table, list view, scroll view, scrolled area),
  the scroll bar no longer crosses the border or pokes out at a corner: its
  track is shortened to clear the rounded corners, both at rest and when it
  unfolds with its arrows. The thumb is also no longer cut off at the corners,
  and the bar in the font-size list no longer overlaps the panel's border.
- **The tab indicator sits on the tab strip's rule.** The blue bar under the
  selected tab floated a few pixels above the grey line, and the hover wash
  stopped short of it. Both now reach the line, as on the web.

- **Live tab strips behave the same everywhere.** A new `TabsController` owns
  what a tab strip keeps between frames — the indicator sliding from tab to
  tab, the scroll arrows, the wheel, the arrow keys and bringing the selected
  tab into view — so every strip gets the component's full behaviour instead
  of a hand-copied part of it. The component gallery's own page navigation now
  uses it: its indicator slides, including on Ctrl+Tab, and the strip answers
  the arrow keys once focused.

- **Floating surfaces never leave a black or stale rectangle behind.** When one
  floating surface replaced another (a tooltip giving way to a menu, say), the
  reused window could briefly show its old or blank content. Tooltips and
  interactive popups now use separate windows, and a popup is only shown once
  its new content has reached the screen.

- **Components no longer paint an opaque square where the web is
  transparent.** A widget drawn straight onto a window or into a floating
  surface filled its box with a background colour; it now lets the surface
  behind it show, as on the web. Text measurement also uses a finite layout box,
  so its metrics are exact.

- **Keyboard focus in dialogs looks like the web's.** Buttons show one 2 px
  accent ring, 1 px off the button, when reached with Tab — dialogs used to
  paint a second ring over the button's own.

- **Menus keep their keyboard highlight.** After moving through a menu with the
  arrow keys, moving the pointer off the menu (or over a separator, a heading
  or a disabled item) no longer clears the highlighted item; only pointing at
  another item changes it, as on the web.

- **Wide data tables no longer hide their last row.** A table wider than its
  container keeps a strip under the rows for its horizontal scroll bar, as the
  web table does.

- **In the component gallery**, the composition page now uses the components'
  own sizing and floating-surface APIs (nothing hangs out of its cards, labels
  no longer run under switches, long words wrap, focus rings stay whole, and
  its drop-downs, calendar and menu escape the scrolled form); a scene opens
  directly with `--scene <number|name>`. A dialog opened with a mouse click no
  longer closes in the same instant; the popover and font-picker demos no
  longer cover or overflow their neighbours.

- **Spinners turn smoothly.** The moving arc of a spinner — and of a button's
  loading ring — was drawn as a row of small dots, which read as a beaded edge
  that shimmered as it turned. It is now one anti-aliased stroke with round
  ends, smooth at every size and angle. The component gallery's spinner row is
  also spaced so the large rings no longer touch.

- **Kubuno windows follow the dark theme everywhere.** A window with the Kubuno
  title bar filled any area a page left unpainted with the light Windows form
  colour, so in the dark theme the component gallery's tab strip (transparent,
  as on the web) stayed light. Such windows now start from the theme's page
  colour, and the gallery's tab pane paints its own ground.

- **Soft shadows no longer turn into a grey slab under raised surfaces.** Web
  shadows such as `shadow-lg`/`shadow-xl` shrink their shape before blurring
  it (a negative spread); the desktop ignored that, so a deep shadow showed as
  the whole surface's silhouette sliding out below it. The shrink is now
  applied, and those shadows fade out softly as they do in the browser.
  Existing shadows are unchanged.

- **Kubuno-chrome windows resize from every edge again, not just the top.** With
  the custom title bar, the host answered the frame hit-test for the top edge
  only, so a window could be resized from its top but not its sides, bottom or
  corners. The host now reports the left, right and bottom edges and all four
  corners too, so a window with the Kubuno caption (the component gallery, and in
  time the shell and Documents) resizes like any other. The caption buttons keep
  priority over the border they sit beside, so min/max/close stay clickable at
  the top-right corner.

- **The gallery tooltip is no longer hidden under the tab strip.** The
  pointer-following tooltip on the display page was painted onto the page, so the
  navigation strip — drawn last, over the top edge — covered it whenever the
  cursor rose toward the tabs, and it could never leave the window. It is now a
  floating surface in its own top-level popup: it sits above everything and
  overflows the window when the pointer is near an edge, the way a tooltip
  should.

- **The font picker no longer freezes a window that shows it open.** Building
  the open list classified every installed family (~260 of them) against three
  keyword lists, and it did so afresh dozens of times per repaint — with the
  host repainting on every pointer move, a debug build pinned a CPU core and
  stopped responding. The classification of a family is now memoised (it never
  changes), the open list is cached until its inputs actually change, and the
  de-duplication of families is no longer quadratic. A page that shows the
  picker open dropped from 100 % of a core to idle.

### Security

- **No more tokens in plain files.** Refresh tokens leave `creds.json` for the OS credential store, and the file sync
  no longer refreshes tokens on its own (a single owner, so two programs can no longer rotate the same token and get
  the whole session revoked). An app checks that the token broker it talks to is the installed Kubuno shell running as
  the same user, and refuses another program squatting the pipe while the shell is not running.
- **The desktop no longer ships a vulnerable XML parser.** The Windows toast
  notifications were built by a library that embedded `quick-xml` 0.37, which
  two advisories (RUSTSEC-2026-0194, RUSTSEC-2026-0195) report as vulnerable to
  denial of service: a crafted document could pin a CPU core with quadratic
  attribute checking, or exhaust memory through unbounded namespace
  declarations. The notification library is now pinned to a release that parses
  no XML at all, so the parser has left the desktop entirely. `cargo audit`
  reports no known vulnerability for the desktop workspace.

### Added

- **A native desktop Documents app (work in progress).** A new `documents`
  binary opens a word-processor window modelled on Microsoft Word's desktop
  layout — title bar, ribbon tab strip, ruler, a paginated page canvas on a grey
  backdrop, and a status bar showing the current page and the zoom. The page is
  drawn directly with Direct2D and DirectWrite; the chrome around it comes from
  the shared Kubuno design system, so it matches Drive, the shell and Chat. Its
  window and taskbar icon is the Documents module's own logo, not the generic
  Kubuno mark. This first slice renders a built-in document: it paginates,
  scrolls with the wheel, the keyboard (Page Up/Down, Home/End, arrows) and a
  scroll bar you can drag, and zooms with Ctrl+wheel. It also opens a real
  document: pass a `content_json` file on the command line and it renders its
  headings, paragraphs and character formatting. The status bar names anything
  the page cannot draw yet ("Non affiché : 5 image, 4 table"), because a
  document that silently appears to be missing its tables reads as data loss
  rather than as an unfinished feature. Typing in a document and saving it come
  next.

- **Documents now shows the full ribbon, matching the web editor.** The empty
  band under the tabs is replaced by the real ribbon of the active tab —
  Clipboard, Font, Paragraph, Styles and Editing on Home — declared from the
  same command inventory as the web (`buildDocumentRibbon`), with group labels
  and separators, the Font and Size fields, the Bold/Italic/Underline toggles,
  hand-drawn glyphs where no icon is embedded (A▲, x₂, x², ¶), alignment and
  list controls, and the dialog launchers. Tabs switch, controls highlight on
  hover, and the zoom commands work; the commands that edit the document light
  up their buttons and act once the editing path is wired. The tab strip and the
  ribbon are now driven by one declaration rather than two lists that could
  drift apart.

- **The pieces a Documents editor is built from.** Not yet reachable from the
  window — they are written, tested and waiting to be wired: the caret and
  selection geometry, undo and redo (typing a word is one step), the Windows
  clipboard payloads, the Word-style ribbon declared as data, the typed client
  for the office routes, the open-document session with its save guard, list
  numbering, table layout, image geometry with an insert-time size budget, and
  the harness that checks this engine against the browser. A click will map to
  the right place in the document even after a table or a list the editor cannot
  yet draw: each on-screen paragraph remembers which stored block it came from,
  and each run remembers its offset in UTF-16 code units, so an edit lands where
  the user pointed rather than one block or one character off.

- **Documents breaks lines the way the web editor does, not the way a text
  engine would.** Paragraphs are laid out by a transcription of the web's own
  engine: the same tokenizer, the same wrap loop, the same trailing-space and
  justification rules, the same absolute 48 px tab grid, and the same
  three-branch line height. DirectWrite is used only to measure and to draw.
  The result is that the same document paginates identically in the browser and
  on the desktop — a difference there is invisible on screen and obvious the
  moment somebody prints. Paragraph alignment, line spacing and paragraph
  spacing are read from the document, and a paragraph longer than a page is
  split across pages line by line instead of jumping to the next page whole.

- **Documents reads and writes the stored format without losing anything.** A
  document is carried as the bytes it arrived as, and only the small part the
  app understands is parsed, so a save cannot drop what this version does not
  model — tracked changes, anchored comments, header and footer definitions, or
  a rich text box's entire sub-document hidden inside an image attribute. Two
  storage shapes are handled and each is written back as it was read: a bare
  ProseMirror document stays bare (one stored document in five is bare, and
  promoting it would change its storage shape and mint new identifiers behind
  the user's back), and a multi-page envelope keeps its first page's identity.
  Proven by a round-trip suite that asserts a real document re-serialises to the
  identical bytes.

### Fixed

- **Documents no longer over-wide-word overflow, mis-scroll when zoomed, or
  place the caret one position off at a line wrap.** A word made of an odd
  number of emoji stayed on one line and spilled past its column (in a table,
  into the next cell); it is now broken like every other over-wide word. A
  zoomed document could not be scrolled to its last page — the scroll offset was
  not scaled by the zoom. And a caret at the end of a wrapped line resolved to
  the wrong side of the dropped space, which would have inserted text one
  position too far right on every wrapped line once editing is wired.

- **Documents will not lose a save or the last few seconds of typing.** The
  idempotency key that guards saves was derived from a value that stayed
  constant across reopens, so reopening a document could make the server replay
  an old save and silently drop retyped content; it now carries real entropy per
  open. And closing a document while a save was blocked (a conflict) or the
  network was down wrote nothing to the local crash journal; it now flushes the
  journal on close, so the next open recovers what was typed.

- **Documents now matches the web editor's heading and paragraph metrics.**
  Heading sizes, per-level spacing, and body spacing follow the web engine
  exactly; headings 5 and 6 are no longer bold; a paragraph's left, right and
  first-line indents and its tab stops are read (they were silently dropped, so
  indented paragraphs wrapped at the wrong width); and an unstyled run measures
  in the web's default font.

### Changed

- The desktop workspace now builds `serde_json` with the `raw_value` and
  `float_roundtrip` features. `raw_value` is what lets Documents carry a subtree
  through untouched; `float_roundtrip` stops a stored value such as
  `47.999999999999996` — the shape a DOCX import's twips-to-pixels conversion
  produces — from being written back as a different number. Features are
  additive across a workspace, so the other desktop binaries build with them
  too; neither changes behaviour for code that does not use them.

- **A native desktop Chat module (work in progress).** A new `chat` binary
  renders a two-pane messenger — a searchable conversation list on the left, the
  open conversation (header, message bubbles, composer) on the right — natively
  in Win32/Direct2D, drawn with the shared Kubuno design system (`kubuno-ui`'s
  buttons, fields, badges and empty states) so it matches the shell and Drive.
  Its window/taskbar icon is the Chat module's own logo (built from
  `chat-logo.png`), the same convention Drive follows — not the generic Kubuno
  mark. It mirrors the Kubuno web chat's layout and bubble rules. It now loads real
  data from the server — the conversation list (`GET /api/v1/chat/conversations`)
  and each opened conversation's messages (`GET …/messages`), decoded from the
  web's base64url envelope — through `kubuno-sync` (which gained generic
  `get_json`/`post_json` helpers for core-proxied module routes), off the UI
  thread. You can type in the composer and press Enter to send (an optimistic
  bubble appears, then the server confirms). Messages now arrive **live** over a
  WebSocket to `/api/v1/chat/ws` (blocking `tungstenite`, on its own thread,
  reconnecting with back-off): an incoming `new_message` appends to the open
  thread and refreshes the list preview, dropping the echo of a message we just
  sent (by id, or by matching the optimistic copy). Opening a conversation marks
  it read (`POST …/read`, clearing its unread badge); presence and typing come
  over the same socket — an online dot and status on a direct contact, an "en
  train d'écrire…" note in the header (auto-clearing on a timer); list rows show
  relative times (Hier, weekday, date); and a meeting conversation carries a
  "Réunion" marker. The window wears its own title bar: the system caption is
  stripped and the app draws to the top edge (WhatsApp/Discord-style) with its
  own minimise/maximise/close buttons and a draggable strip. An offline sample
  stands in until the account and list resolve. The in-call (WebRTC) UI follows.

- **`kubuno://` meeting hand-off (web → desktop, Zoom-style).** The chat app
  registers a `kubuno://` URL protocol pointing at itself, so the web can hand a
  meeting to the desktop: a `kubuno://meet/<id>` link opens (or raises) the app
  on that conversation. It is single-instance — a launch carrying a link forwards
  it to the already-running window (`WM_COPYDATA`) and exits, so a hand-off never
  opens a second chat; the running window comes to the front and opens the
  meeting. (The web still needs to emit the link, and the in-call UI follows.)

- **The app launcher shows the server's branded logos.** Modules the desktop
  ships no built-in logo for (photos, forms, media, tasks, forum…) used to fall
  back to a flat single-colour glyph, while the web showed each module's real
  branded logo. The launcher now displays a logo the server serves for a module
  (a `logo_url`/`logo` on `/api/v1/modules`): it is downloaded and cached like
  the profile photo, decoded once, and drawn on the tile in place of the glyph —
  so a logo added on the server appears on the desktop with no update. The
  module's own tile wears the module logo; a sub-app (Documents, Watch, Vertex…)
  keeps its glyph unless the server serves a logo for that entry — the web's
  `moduleGlyph` rule. Modules with no server logo keep their built-in logo or
  glyph, exactly as before.

- **Security policy and CI quality gate.** A `SECURITY.md` documents how to
  report vulnerabilities, and a CI workflow enforces `clippy -D warnings`, a
  dependency-vulnerability audit (`cargo audit`) and the frontend typecheck/tests.

### Changed


- **The desktop apps now use the operating system's default UI font.** The Drive
  app, the shell and the Chat client rendered their text in an embedded typeface
  (Plus Jakarta Sans); they now read in the system's own UI font (Segoe UI
  Variable on Windows 11, falling back to Segoe UI), so they match the rest of
  the desktop and no longer ship the font files. A font chosen in settings still
  overrides it. (Shared change in `drive-app-controls`, so it applies to all
  three at once.)

- **The README now opens with the Kubuno logo.** The public README on GitHub
  shows the Kubuno crest at the top of the page. The image ships in-repo, under
  `.github/logo.svg`, so it renders even when the repo is browsed offline.

- **The sync client now sends a `User-Agent`.** Its HTTP requests carried
  reqwest's blank default, so the server could only tell the desktop client's
  uploads apart by IP. It now announces `Kubuno-Desktop-Sync/<version> (<os>)`,
  like the other Kubuno clients — the OS read at runtime, not hard-coded.

### Fixed

- **Sync never trashes files a server-side reorganisation moved.** When the
  server repathed a folder whose files shared their bytes (same content) with
  files in another folder, the desktop could delete the moved-away copy on disk
  and then push that as a deletion — trashing the still-valid file on the server.
  Two guards close it: a file whose exact content still exists on disk under
  another tracked path is treated as reorganised, never deleted (so its removal
  is never pushed), and a move no longer deletes a local file that a second entry
  still points to. "Absent from disk" no longer means "the user deleted it".

- **Sync no longer duplicates files or undoes server-side moves.** When a file was
  moved or renamed on the server, the desktop wrote it to its new local path but
  left the old copy on disk, untracked. The next push saw that orphan as a brand-
  new file and re-uploaded it; the server appended " (2)" on the name collision,
  that copy was pulled back and re-uploaded as " (2) (2)"… an unbounded cascade
  that also refilled the drive root and reverted any reorganisation done on the
  server. The old local copy is now removed once the file exists at its new path.

### Added

- **`kubuno-ui`: the Kubuno design system, rebuilt on the WinForms replicas.**
  A Kubuno primitive is no longer a control written from scratch — it is a
  replica from `kubuno-controls`, kept whole for its model, and repainted in the
  Kubuno look. The property surface, the defaults, the state machines (three-state
  check cycling, scroll-bar arithmetic, list selection with its `Ctrl`/`Shift`
  rules, tab selection) and the layout engines (Dock/Anchor, Flow, Table, Split)
  are the ones already checked against the real toolkit; only the pixels are new,
  and they come from theme tokens rather than from any colour literal.

  Eight families ship at once: buttons, text, lists, ranges, containers,
  navigation, display and views — about sixty types, 205 tests. The desktop
  gains the controls it simply did not have: a real **container** that places
  its children through the layout engine, check boxes and radios, combo boxes,
  tabs, group boxes, sliders, progress bars, badges, tooltips, list and tree
  views with virtualisation.

  Where a primitive replaces a hand-written predecessor, it had to be
  indistinguishable from it before anything adopted it: the gallery paints the
  old and the new side by side from the same inputs, and the two halves of a
  single screenshot are diffed pixel by pixel. Buttons came out at **zero**
  differing pixels across six variants and four states.

- **`kubuno-ui` text primitives**: `TextField`, `TextArea`, `SearchField` and
  `MaskedField`, each wrapping its WinForms replica (text, read-only, max
  length, password character, multiline, alignment and the whole mask engine
  come from there) and adding the Kubuno look — icon columns, a placeholder, an
  invalid state, the accent focus ring. Editing itself is unchanged: the caret,
  the selection band and the scrolling that keeps the caret in view are still
  the shared editor the shell and Drive already use, so a focused plain field
  is pixel-for-pixel the one that ships today. The search field is the omnibar's
  pill without its mode buttons, at the omnibar's own measurements.

- **`kubuno-ui` range primitives**: `ScrollBar`, `Slider`, `ProgressBar`,
  `NumericField` and `DomainField`, each wrapping its WinForms replica and
  painting the Kubuno look. The scroll bar is a drop-in replacement for the
  hand-written one the shell and Drive use today — same rail, same thumb, same
  chevrons, proven by a geometry-equality test — and it now takes its
  hover colour from the theme instead of a hard-coded pair of hex values.
  Progress bars gain the web's automatic warning/danger thresholds and its
  indeterminate sliver; numeric fields make the "set `Maximum` before `Value`"
  ordering trap unreachable.

- **`kubuno-ui` list primitives**: `ListBox`, `CheckedListBox`, `ComboBox` and
  `Menu`. Each wraps its WinForms replica and keeps its whole model — the items,
  the four selection modes with their `Ctrl`/`Shift` rules, `TopIndex`,
  multi-column layout, `Sorted`, `MaxDropDownItems`/`DropDownHeight`, and the
  per-item check states — while painting the Kubuno look. `Menu` **is** the
  product's `MenuDropdown`, rebuilt to the pixel: 30 DIP rows, a full accent
  hover pill, icons and shortcuts in their own aligned columns, section labels,
  inset separators, checked and destructive entries, and cascading submenus.
  Lists and menus can now say *which* row a point lands on, so a host hit-tests
  before it dispatches instead of guessing.

- **`kubuno-ui` data table**: `DataTable`, the desktop port of the product's one
  table. It is a `ListView` — the same columns, rows, selection machine, sort
  direction and virtualisation — wearing the web table's chrome and ink:
  sortable headers with the ascending/descending/none cycle, a selection column
  whose « select all » box is genuinely three-state and covers the current page
  only, a bulk-action bar that replaces the toolbar as soon as something is
  selected (three actions inline, the rest folded into an overflow), a
  pagination footer with a page-size selector and first/previous/next/last
  jumps, the three empty states (nothing yet, no result, load failed) and a
  loading skeleton. In a container under 700 DIP the rows become **cards** — the
  primary column titles each one and the others become label/value pairs —
  through a pure `layout_mode(width)` rather than a branch buried in a painter.
  A table of 100 000 rows with pagination off still paints exactly a screenful.

- **`kubuno-ui` feedback primitives**: `Spinner`, `EmptyState`, `Callout`,
  `Accordion` and `Stepper` — the five surfaces that tell you what is going on,
  none of which the desktop had. They are assemblies rather than new controls:
  every text run is a `Label` at a step of the web type scale, every glyph is an
  `Icon` (geometry, never a character), a section's count is a `Badge`, a
  section's card is a `Panel` on the card surface and an empty state's buttons
  are real `Button`s — so a title painted here and a title painted anywhere else
  in the system are the same title.

  `Spinner` is the family's one animated primitive and it owns no clock: the
  phase is a parameter the host advances, exactly as the indeterminate progress
  bar's already is, so the same inputs always paint the same picture and the
  whole turn is capturable in one screenshot. `Callout` carries the four
  severities on a tinted ground (the accent stays on the mark, where contrast is
  not a legibility requirement); the four lucide marks it needs — `Info`,
  `CheckCircle2`, `AlertTriangle` and `AlertCircle` — join the shared icon
  geometries. `Accordion` publishes its section rectangles,
  its hit test and its total height as pure functions of what is open.
  `Stepper` carries all five step states — including **error**, which is what
  makes the control more than a decoration: a wizard whose third step failed
  says so on the indicator instead of at submit time.

- **`kubuno-ui` dialogs**: `FloatingWindow`, `ConfirmDialog`, `PromptDialog`,
  `ConflictDialog`, `Popover` and `Toast` — the family the product's own rule
  requires (« never a browser dialog: use `ConfirmDialog` / `PromptDialog` »),
  which until now had no desktop counterpart at all, so a shell that needed a
  confirmation had nothing to call. Each is a port of the web component that
  ships: the accent title band, the one footer the whole product shares — the
  action on the left, the cancel on the right, both at least 96 wide, so a
  hundred dialogs cannot each have an opinion — the modal veil that closes on a
  click outside, and the three-way name conflict Drive shows on a collision.
  A dialog **measures** itself from its own message (wrapped to its width) and
  is **placed** by a pure function that keeps it inside a host smaller than
  itself; a popover chooses its side and folds back at each of the four edges,
  reusing the tooltip's placement rather than growing a second copy of it; a
  stack of toasts is a pure function too, dropping the oldest past four.
  Nothing is rebuilt that already existed: the bands come from the layout
  engine, the buttons from `kubuno-ui`'s own, the prompt's input from
  `TextField` — caret, selection and all.

- **`kubuno-ui` colour primitives**: `ColorField`, `ColorPicker`,
  `SwatchPicker` and `GradientPicker` — the family WinForms has no control for
  (it ships a `ColorDialog`, not a widget), so the reference is the web alone.
  The swatch button wraps the button replica, every panel wraps the container
  replica, and the hue, alpha, angle and opacity sliders wrap the track-bar
  replica, so a thumb's position and the value it names come from arithmetic
  that was already checked against the toolkit.

  The colour maths is a faithful port of the web's own `color.ts` and
  `gradient.ts` — RGB ⇄ HSV ⇄ HSL ⇄ CMYK, the hex and `rgba()` serialisers, the
  gradient's stop sampling — **rounding included**: it runs in double precision
  and breaks ties the way JavaScript does, so no channel can come out one unit
  from what the browser shows. Every one of the ninety-two colours the two
  shipped palettes contain survives a round trip through all three colour
  spaces. The notations read are the ones the web writes (`#rgb`, `#rrggbb`,
  `rgb()`/`rgba()`), plus `#rrggbbaa` and `hsl()` so a colour with an alpha has
  a notation at all; everything else is refused rather than guessed at.

  Two limits are worth knowing, because the drawing surface has no gradient
  brush of any kind: the saturation/value area is built from the browser's own
  three-layer recipe (a hue ground, a white ramp across, a black ramp down),
  which is exact but quantised to one DIP per strip, and the gradient preview
  is a grid of 2 DIP cells sampled from the gradient, so an angled or radial
  one is visibly banded. The web's hue RING is not reproduced at all — there is
  no conic brush — and the panel uses the same component's hue **slider**
  instead. No contrast check is offered, because the web computes none.

  Four theme tokens were added for this family's chrome — the two squares of
  the transparency chequer and the two strokes of a picker handle. They are
  deliberately identical in the light and dark palettes, because the web writes
  them as literals: a chequer and a handle outline have to stay readable over
  whatever colour the user has just picked, so they do not follow the surface.

### Changed

- **Drive's command bar, path trail and status bar now paint with
  `kubuno-ui`.** The three bars kept their own description of themselves — a
  flattened list of "visuals" for the paint on one side, a hand-rolled walk of
  rectangles for the hit test on the other — and the two could, and did, drift
  apart. Each bar is now a design-system primitive that owns both: the commands
  are `Toolbar`'s `ToolStripItem`s (so a greyed-out command, a toggle's on
  state, a separator and a drop-down are the replica's own fields, stored once),
  the path is `Breadcrumb`, which wraps the very folding algorithm the layout
  pass already used, and the count-and-selection line is `StatusBar`, whose
  `Spring` cell hands the leftover width to the summary and packs the git
  widgets against the trailing edge. The box the pointer hits is now, by
  construction, the box that was drawn.

  Visible differences: the « + » of « Nouveau » and every chevron are drawn from
  the design system's own geometry rather than from an icon-font codepoint, so
  they match the web's line weight; commands are spaced by the toolbar's own
  4 DIP throughout instead of 8 here and 6 there; and « Nouveau » is now as wide
  as its caption needs in the current language instead of a fixed 118.

- **Drive's file-operations flyout uses the design system's empty state.** With
  no operation in flight, the panel drew its own medallion and title; it now
  paints `feedback::EmptyState`, so the disc, the glyph size and the spacing are
  the ones every other empty collection in Kubuno uses.

- **Drive's settings pages now paint with `kubuno-ui`.** The rows of every
  settings section had a hand-written copy of the design system in them, written
  before the primitives existed: its own two-variant button, its own selector,
  its own switch, its own warning box and its own keyboard-shortcut chip. Each
  is now the shipped primitive — `Button`, `Dropdown`, `Switch`, `Callout` and
  `Badge` — and, more importantly, each is now **measured** by the primitive
  against the real font instead of by a per-character estimate. Buttons and
  chips therefore hug their label exactly, in every language, rather than being
  a few pixels wide or narrow; the selectors gain the design system's solid
  caret in place of a font glyph, and fill on hover as the web's does; the
  warning box carries the real `warning-light` ground and the `AlertTriangle`
  mark rather than a tint derived from the warning colour, and its row is three
  pixels taller because that is what the callout actually measures. What the
  pages themselves are — the cards, the expanders, the group headers — is
  untouched.

- **The background-colour picker of Drive's appearance settings is now the
  Kubuno colour panel.** The flyout behind the "Background colours" button used
  to be a port of the WinUI/Files `ColorPicker` — two tabs, a hue×saturation
  square and a separate value slider. It is now `kubuno-ui`'s `ColorPicker`:
  a saturation/value area, a hue slider, an alpha slider, a live preview over
  the transparency chequer, the hex value and twelve one-click colour chips.
  Picking a chip applies its colour immediately and keeps the opacity you set.
  The per-channel R/G/B tab is gone; the chips and the hex read-out replace it
  for the exact-value cases. Dragging works as before on the area and on both
  sliders, and the colour still applies live to the window background.

- **The shell now paints with `kubuno-ui`.** All 149 button call sites across
  thirteen files moved from the hand-written control to the rebuilt one. The
  rendering is unchanged by construction — the replacement was proven
  pixel-identical first — so this is a change of foundation, not of appearance:
  buttons in the shell now carry the full WinForms model (enabled state,
  padding, alignment, auto-size) instead of a bespoke struct.

- **The administration console's Groupes, Unités organisationnelles and
  Audiences cibles sections say « loading », « nothing here » and « that
  failed » properly.** A grey line of text has become a spinner with a caption,
  a medallion with a title and a sentence explaining what would appear there,
  and a danger callout. The badges on those pages — « Par défaut », « Système »
  and the permission chips of an expanded group — are now the design system's
  own badge, measured rather than sized by hand, so a long label no longer
  overflows its pill.

- **The Tableau de bord, Applications, Instance and Stockage sections, and the
  Activité page, say the same three things the same way.** « Chargement… »,
  « Aucune donnée. » and a red line of error text have become a spinner with a
  caption, a medallion with a title and a sentence saying what would appear
  there, and a danger callout. On Applications, the service-state dot and its
  wording are now one design-system badge — green while the module is on, quiet
  neutral while it is off — and the « Par défaut » pill is measured rather than
  sized by hand.

- **The bars on the Stockage and Tableau de bord pages are the design system's
  progress bar.** The per-unit and per-category bars, and the share of active
  accounts, are drawn by the shared primitive instead of by three separate
  hand-rolled fills. They keep a **stated** colour rather than the component's
  automatic quota thresholds: these bars measure a row against the *largest*
  row, not against a quota, so amber at 75 % and red at 90 % would flag the top
  row of every card for being the top row. The instance's real threshold is
  still the states card's « seuil N % ». The composition bars — accounts against
  the volume, and the account-state tally — stay hand-stacked, since they carry
  several segments where a progress bar carries one; they now share the
  primitive's track thickness, so a card no longer shows two different bars.

### Added

- **`--page <name>` opens a page directly at start-up** (`--page admin:storage`).
  The administration console was otherwise unreachable without a mouse — avatar,
  then menu entry, then rail row — which made every one of its sections
  impossible to capture from a script, and is how a whole console came to ship
  checked only by its tests. The request is held until the identity arrives,
  because the shell closes the console for a non-administrator and the first
  identity message would otherwise slam it shut again.

### Fixed

- **A server-side folder MOVE no longer orphans its old directory.** The pull
  reconciled a moved/renamed FILE (write the new path, drop the old) but not a
  moved FOLDER: it created the new directory and left the old one on disk,
  untracked — the folder-level twin of the file cascade, and the source of the
  `Vidéos (2) (2)` / `Livres (2) (2)` duplicates. The old directory is now
  removed once the file pass has emptied it, and ONLY if empty (`remove_dir`,
  never `remove_dir_all`), so an un-reconciled child is never deleted with it.
  Tested.

- **Deleting a folder still did nothing in some cases** — the previous fix for
  this left two holes. Its « a folder is only deleted when the directory that
  should contain it is present » guard backfired on the common case: deleting a
  whole subtree removes that containing directory too, so the guard queued
  nothing. And trashing a folder dropped only its own row from the local index,
  orphaning every descendant — present in the index, gone from disk and server,
  and no longer detectable because their own parent was now gone and untracked.
  The guard now walks up to the nearest surviving, READABLE ancestor (which
  tells a real deletion apart from an unreadable drive), and the index removes a
  folder together with its whole subtree. The sync push had no tests at all;
  both paths now have them.

- **Round buttons no longer respond in their corners.** Every page derived a
  square rectangle for a circular icon button and hit-tested that square, so the
  pointer activated a pencil or a close button while visibly outside the circle.
  The rule now lives in one place (`kubuno_ui::buttons::circular_hit`), shared by
  the control's own hit test and by the pages that test a rectangle without
  building the control, and it matches the web, where `border-radius` clips
  pointer events too.

### Removed

- The shared-DLL machinery: `-C prefer-dynamic` (`windows/.cargo/config.toml`), the link shim of `kubuno-ui`'s
  `build.rs` that named each build `kubuno_ui-<hash>.dll`, `kubuno_ui::library`, the `library_file_name` test,
  `tools/stage-runtime.ps1`, and the DLL shipping of `packaging/package-msix.ps1` (which now refuses an exe that
  still imports a Rust DLL). `tools/build-all.ps1` only builds.
- The Visual Studio design surface (`kubuno-views/examples/view_embed.rs`) no longer reports the DLL it loaded: its
  `surfaceInfo` handshake is version 2 and carries no DLL path or hash.
- **Tauri**. The desktop application is now a native Win32 shell (`shell/`,
  binary `kubuno-desktop`) drawn with the Drive components — no web view, no
  WebView2, no bundler. The `app/` crate is gone, and with it the **28 IPC
  commands**, the **local caching proxy** and its axum/reqwest/tungstenite
  dependencies, the per-document native windows and the `desktop_bridge_js`
  injection. Modules open in the user's browser. The binary went from 20.5 MB
  to 10.5 MB.
- **Local WASM backends**: the leftover `wasmtime` wiring, the local module
  backends and the component manifest, following the core dropping
  `GET /api/v1/desktop/wasm`.
- `get_instance_modules` (module-contributed desktop settings) was NOT ported:
  the `/api/v1/desktop/modules` endpoint does not exist in the core and no
  module declares such settings — it always returned an empty list.

### Added

- **`kubuno-controls` now paints the real themed Windows parts.** Controls used
  to be drawn entirely from `GetSysColor` and a `DrawEdge` recipe — a coherent
  and correct *classic* look, but not the one the reference sheets show, because
  the toolkit renders them with visual styles **on**. Pixel-sampling the sheets
  found colours that exist in no system colour at all: a `#ABADB3` text-field
  frame, a `#FDFDFD` button face over `#D0D0D0`/`#BABABA` edges, a `#DCDCDC`
  group-box line.

  A new `theme` module renders those parts through `uxtheme.dll` itself and
  hands them to Direct2D, so the result is identical **by construction** rather
  than by approximation. Text fields (`BorderStyle.Fixed3D`) and buttons
  (`FlatStyle.System`) use it now; the other families follow. Nothing is lost on
  a machine with visual styles switched off, or under a high-contrast theme: the
  classic painting stays and is used automatically, and `KUBUNO_CONTROLS_CLASSIC`
  forces it on a themed machine for comparison. The rendered parts are cached
  per size and per DPI — a redraw costs well under a microsecond — and rebuilt
  when the display scale or the desktop theme changes.

- **`kubuno-controls` — the primitive control library.** A native reproduction
  of the WinForms control surface on Direct2D, meant to become the one set of
  primitives every Kubuno desktop application builds on. 65 control types were
  read out of the shipping `System.Windows.Forms` assembly **by reflection**
  (inheritance chain, every designer-visible property with its declared default
  and description), and the real toolkit rendered ten per-family reference
  sheets in the states that change how a control paints. Both generators are
  checked in under `tools/winforms-ref/`, so the reference is reproducible.

  The **inheritance chain is the design**: `Control` declares 52 settable
  properties inherited by all 64 other types, `ButtonBase` adds 18 shared by
  three controls, and `Button` itself declares two. The port mirrors that with
  **composition + `Deref`**, so each type owns exactly what its .NET counterpart
  declares — including the subtlety that a subclass often *re-declares* a base
  property only to change its default (`RadioButton` starts `TabStop = false`),
  which gets a new default but never new storage.

  Implemented, each with a documented property table and tests: `ButtonBase →
  Button / CheckBox / RadioButton`; `TextBoxBase → TextBox / MaskedTextBox /
  RichTextBox`; `ListControl → ComboBox / ListBox → CheckedListBox`;
  `ScrollableControl → ContainerControl → Form / UserControl`, `Panel`,
  `GroupBox`; `FlowLayoutPanel`, `TableLayoutPanel`, `SplitContainer`,
  `Splitter`, `TabControl / TabPage`; `Label → LinkLabel`, `PictureBox`,
  `ProgressBar`; `ScrollBar → H/V`, `TrackBar`, `UpDownBase → NumericUpDown /
  DomainUpDown`; `DateTimePicker`, `MonthCalendar`; `TreeView`, `ListView`;
  and the `ToolStrip` family with its separate `ToolStripItem` hierarchy.
  Layout (`Dock` then `Anchor`, flow, table, split, tab strip) is resolved by
  pure functions, so container behaviour is tested without a window. A property
  that is not yet honoured says so on its own field rather than being dropped or
  quietly treated as another value. `DataGridView`, `PropertyGrid`, `WebBrowser`
  and the `AxHost` interop family are deliberately left to a later wave.

  The library ships its own host (`host::run` + a reusable `Painter`
  implementing `Canvas`), so it can be driven without the shell, and a
  **gallery example** (`cargo run -p kubuno-controls --example gallery`) that
  rebuilds each reference sheet out of the real controls, so the port and the
  toolkit can be put side by side (`tools/winforms-ref/compare.ps1`, which
  normalises the two captures to one scale — the reference is written in logical
  pixels and a screen grab is physical, and composing them raw would show a fact
  about the capture as if it were a fact about the port).

  Controls paint through `Control::paint` at rest and through the provided
  `paint_with_state(…, ControlState { hot, pressed, focused, default })` when
  the host knows what the pointer is doing — which is what makes
  `FlatAppearance.MouseOverBackColor`, `LinkBehavior::HoverUnderline` and the
  `Popup`/`System` flat styles expressible at all, since those differ *only*
  under the mouse. The state is a provided method, so a family with nothing to
  show under the pointer implements nothing.

  **The controls are painted as faithful .NET replicas**, not in the Kubuno
  design system — square corners, real 3-D edges, and every colour, metric and
  font read from Windows rather than chosen: `GetSysColor` for the palette,
  `GetSystemMetricsForDpi` for scrollbar widths and edge thicknesses, and
  `SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS)` → `lfMessageFont` for
  the UI font, which is the same mechanism .NET uses for `Control.DefaultFont`
  (Segoe UI 9 pt = 12 DIP on a default install, and whatever the machine says
  elsewhere). The Kubuno skin becomes a later layer on top of these replicas.

  Repainting against the reference is what caught the metrics that had been
  quietly taken from the Kubuno design system instead of the toolkit:
  `ListBox.ItemHeight` is 15 and not 20, a `CheckedListBox` row is 18, an item's
  text inset is 2 and not 6, and a line box is `2724/2048` of the em — Segoe UI's
  own `hhea` spacing, which .NET agrees with to the hundredth. It also caught
  three colour traps worth naming: a `LinkLabel` is **not** `COLOR_HOTLIGHT`
  (the toolkit resolves the link colour through Internet Explorer's setting), a
  disabled label is `COLOR_BTNSHADOW` and not `COLOR_GRAYTEXT`, and
  `COLOR_GRAYTEXT` appears nowhere in the reference sheet at all.

  The port is checked against the toolkit **mechanically**, by three kinds of
  harness under `tools/winforms-ref/`, so "it matches" is a measurement rather
  than a claim:

  * `audit-coverage.ps1` cross-checks every property the catalogue says a type
    declares against the source — 659 properties, classified as implemented,
    correctly inherited through `Deref`, documented, or missing.
  * `parity/` drives **real WinForms controls** and the port over the same case
    list (declared once, read by both sides, so they cannot drift) and diffs the
    numbers: `Dock`/`Anchor` including nesting and combinations, the layout
    panels' arithmetic, and the range controls' state machines.
  * `compare.ps1` puts a rendered page beside its reference sheet, normalising
    the two captures to one scale — the reference is written in logical pixels
    and a screen grab is physical, so composing them raw would present a fact
    about the capture as a fact about the port.

  The `Dock`/`Anchor` harness alone started at 31 of 52 cases disagreeing and
  found eight defects in the layout engine, none of which the unit tests could
  see — because those tests encoded the port's own understanding rather than the
  toolkit's, and two of them asserted the opposite of what WinForms does. `Fill`
  is resolved in place and consumes nothing (which is *why* the toolkit wants a
  Fill child at the back of the z-order); a band overflows rather than being
  clamped to the space left; `MinimumSize`/`MaximumSize` apply on both axes of a
  docked band; a hidden docked child keeps its bounds instead of being zeroed;
  an unanchored axis recentres, with integer halving, so 201 → 300 moves a child
  by 50 and not 49.5; a hidden anchored child is still anchored; and the display
  rectangle is client-relative, carrying its own padding inset. It now matches
  the toolkit on **52 of 52 cases and 107 of 107 rectangles**.

  A harness that provokes toolkit code through a window procedure must set
  `UnhandledExceptionMode.ThrowException`: an exception thrown inside a
  `WndProc` does not come back out of the `SendMessage` that caused it, so
  WinForms turns it into a modal dialog and an unattended run hangs — which
  makes a stuck run indistinguishable from a green one. It also hid two real
  results, where the toolkit's own scroll handler computes a value and then
  rejects it.

  Building the gallery is what found the defects that unit tests could not: a
  DPI factor applied twice (invisible at 100 %, and it pushed a tool strip's
  last item into the overflow), a host that never initialised COM — so the
  renderer failed and the window stayed blank with the error swallowed — glyphs
  drawn as text characters the embedded face does not carry, child bounds kept
  in canvas space instead of parent-relative, a content rectangle read from the
  control's own field rather than the rectangle it was asked to paint into, and
  a stale cached DPI that let layout measure against a window wider than the one
  it was drawn in.

  Running the real toolkit also settled questions the documentation alone would
  not have: `NumericUpDown.Value` throws outside `Minimum`/`Maximum` (so the
  maximum must be raised first), a scrollbar's highest reachable value is
  `Maximum - LargeChange + 1`, `TreeView` check boxes do **not** cascade, and
  `ProgressBar.PerformStep` clamps rather than wraps.
- A `link_visited` colour token in the shared `Theme` (both palettes), so
  `LinkLabel` paints a followed link from the palette instead of a hard-coded
  purple; and `Check`, `Minus`, `Plus`, `ChevronUp` geometries in the shared
  icon set — a check mark drawn as a text « ✓ » depends on whichever font
  resolves it and would not match at any size.
- **The web shell's chrome**, replicated from `AppHeader` / `HeaderActions` /
  `WaffleMenu`: a 64px header carrying the hamburger, the wordmark, search, the
  storage gauge, notifications, settings, help, the waffle and the avatar, with
  36px round buttons and 18px glyphs. The header is merged INTO the title bar —
  `WM_NCCALCSIZE` drops the system caption while keeping the resize borders,
  `WM_NCHITTEST` makes the band draggable except over a control, and the shell
  draws the caption buttons (the Maximise one still reports as `HTMAXBUTTON`,
  so Windows 11 keeps offering its snap-layouts flyout).
- **Waffle panel** holding the applications, fed by `/api/v1/modules`: each
  module's own brand logo when it has one and its Lucide glyph otherwise — the
  two families the web launcher uses. 360px wide, three columns, scrollable. It
  is hosted in its own popup window with a **blurred backdrop clipped to the
  web's 28px corner radius** — something neither DWM (fixed radius) nor the
  legacy acrylic (rectangle only) can do. The popup builds a
  Windows.UI.Composition tree: a host-backdrop brush (fed by
  `ACCENT_ENABLE_HOSTBACKDROP`, already blurred by the compositor), the
  `--kb-float-surface` tint, the Direct2D swap chain on top, all clipped by a
  rounded-rectangle geometry. The owner keeps the mouse capture, so the popup
  itself handles no input.
- **Account panel** behind the header's avatar, replicating `UserPanel`: the
  same chrome as the waffle (its popup, the tinted ground, the 28px radius, the
  shadow), the email and close on a pinned header, then the hero — a 96px photo
  ringed in the accent, « Bonjour <prénom> ! », and « Gérer votre compte » —
  over white cards. Measured on the running web panel: 320 wide, cards inset
  `mx-2`, 56.6 rows. The avatar used to jump straight to the accounts page.
  What the web calls the other accounts of the browser maps to the shell's own
  notion of an account, the configured instances, so those rows switch instance;
  « Étiquettes », « Administration » and the change-photo button have no desktop
  counterpart and are left out rather than drawn dead.
- **Étiquettes** and **Administration** in the account panel, as on the web. The
  console entry is shown only to an account that may enter it, read from
  `privileges.is_admin` on `GET /api/v1/me` — the same question the web asks,
  which is not the raw role (a delegated administrator is still `role = 'user'`).
- **Labels page** (`Page::Labels`), replicating `LabelsPage`: the list with each
  label's colour, name and link count, creation, recolouring from the web's own
  palette, and deletion — every one of them a call to `/api/v1/labels`, nothing
  cached locally, so the web and the desktop always show one list. Deleting asks
  first: a label spans every module, so it unlabels items the user may not have
  in view. The web's browse pane (the items carrying the selected labels) is NOT
  ported yet, and the page says so rather than showing half a screen.
- **Administration console** (`Page::Admin`), ported in waves. The console's
  navigation lives in the shell's **left rail**, below the main nav and a
  separator, under an « ADMINISTRATION » header — the web's full navigation TREE
  (`adminNav.ts`) verbatim: top-level groups carry an icon and a chevron;
  expanding one reveals its leaves, indented; a leaf selects its section and the
  content pane shows only that section. Groups collapse by default.
  The **Annuaire → Utilisateurs** page is laid out to match the web `UsersPanel`
  list: a breadcrumb and title, a toolbar (search field, count, « Nouvel
  utilisateur »), then the table with the web's columns — Utilisateur (name over
  email), Unité organisationnelle (resolved from `/admin/org-units`), Rôle,
  Quota, Dernière connexion (relative, « il y a N jours » / « Jamais ») and
  Statut — with a per-row pencil that opens the account on the web console. The
  create/edit/bulk actions are not drawn as dead controls; they open the web.
  The **Tableau de bord** matches the web landing: a breadcrumb and title, the
  four summary cards (Utilisateurs total « +N cette semaine », actifs with a
  progress bar and « % du total », connectés « N sessions », modules « N/N
  sains ») each with a top-right icon, the intro line, and two daily bar-chart
  panels (« Connexions par jour », « Nouveaux comptes par jour ») drawn from the
  `logins_daily` / `signups_daily` series. Sections are
  self-contained modules over a documented contract, so they can be built
  independently. Ported so far: **Tableau de bord** (`/admin/stats`),
  **Annuaire → Utilisateurs** (`/admin/users`, paginated table),
  **Annuaire → Groupes** (`/admin/groups`, matching the web `GroupsPanel`: a
  breadcrumb, the title with its count and a « Nouveau groupe » button, then each
  group as an expandable row carrying its « Par défaut » / « Système » badges,
  its description and its member count — expanding it reveals the permission
  badges and the creation date; create, edit and member management open the web),
  **Annuaire → Audiences cibles** (`/admin/audiences`, the web
  `AudiencesSection` table: Nom — with a globe on the seeded « tout le monde »
  audience — Description, Membres, Comptes atteints and Proposée; the sheet and
  create/delete open the web),
  **Annuaire → Unités organisationnelles** (`/admin/org-units`, matching the web
  `OrgUnitsPanel`: the units as an indented tree — root first, each child under
  its parent — carrying the name, the description and the subtree account count
  (summed from `/admin/users?counts=true`); « Nouvelle unité », add-child and
  edit open the web),
  **Applications**
  (`/admin/modules`, laid out to match the web `ModulesPanel`: a breadcrumb and
  title with the count, a « Marketplace » button, a filter box, the
  « Application » / « État du service » columns, and rows carrying the module's
  coloured brand logo — its Lucide glyph otherwise — its name, version, a
  « Par défaut » badge on the module the instance opens on
  (`navigation.default_module`), its description, and the service state as a dot
  and « Activé/Désactivé pour tout le monde » with a per-row ⋮ that flips it),
  **Instance**
  (`/admin/settings`, grouped read-only) and **Stockage**
  (`/admin/storage/overview`, matching the web `StorageSection`: a title and
  intro, an « Aperçu » card leading with the space used against the physical
  volume — a composition bar of accounts / other / free and the volume path —
  over the accounts and allocated figures; an « État des comptes » card with the
  ok/near/full bar and the « seuil N % » it is measured against; the split by
  organisational unit; and the per-category breakdown. A figure the server does
  not send is left out, never estimated). Every section
  not yet ported says so and opens the web console at that exact section. A
  dashboard counter the server does not send is skipped rather than shown as
  zero. The `kubuno-sync` API gained the module, settings and storage endpoints.
  The gate is applied at every layer — the menu row, the entry point, the hit
  test and the drawing — so an administration surface is neither shown nor
  reachable for anyone else, and leaving the console is automatic if the account
  switches to one without access. The server enforces it regardless.
- A native confirmation (`confirm.rs`) for destructive actions, and
  `kubuno-sync` gained the label, dashboard and directory endpoints.
- The waffle's **favourites**, shared with the web: the list is read from and
  written back to `preferences.waffle_favorites` on the account
  (`PATCH /api/v1/me`, whose JSONB merge leaves the other keys alone), so both
  clients show one list. The pencil opens an edit mode with Cancel/OK, click or
  drag to add and remove, drag to reorder with an insertion bar, and the same
  cap of nine. Nothing is written until OK.
  Favourite ids this build cannot resolve are **carried through untouched**: the
  web's own edit path drops what it cannot resolve, and doing the same here
  would have deleted the web's favourites from the desktop.
- The header's avatar shows the account's **profile photo**, clipped to a
  circle, falling back to the initials. The shared `Canvas` gained a rounded
  clip for it, which Drive can use too.
- **Home page** with the three cards the Tauri build had: sync status,
  connection (with the forced-offline switch) and quick links.
- **Sidebar navigation** (Accueil / Activité / Comptes / Paramètres), which
  collapses to icons on a narrow window.
- **Accounts** page: several instances, one current, folder relocation, sign-out
  and the signed-in identity.
- **Activity** page: a bounded log (100 entries) fed by the sync loop and by
  manual syncs.
- **Settings** page: theme, sync interval, notifications, start with Windows,
  forced-offline mode and the outbound proxy.
- **Sign-in** page, with a native folder picker (`IFileOpenDialog`). It is the
  first screen when no account is configured.
- Windows toasts, autostart through the per-user `Run` key, and the Explorer
  integration (CloudFiles sync root + navigation-pane entry) carried over.
- MSIX packaging moved to `packaging/` — it never depended on Tauri.
- Shared controls gained a toggle switch (`switch.rs`, used by Drive and the
  shell), literal-colour and stroked icon layers, layer transforms and icon
  name aliases.
- **A shared `Button`** (`button.rs`), replicating `@ui/Button`: the six
  variants, the three sizes, the hover/active/disabled states and the intrinsic
  content-driven width. Every metric was MEASURED on the running web app over
  CDP rather than read off the Tailwind classes, because the project scales its
  radius ramp — `rounded-md` resolves to **4**, so the web has no pill-shaped
  buttons at all. The shell now draws its buttons through it (home, accounts,
  sign-in, and the waffle's edit controls) instead of hand-rolling 55 rounded
  rectangles across eight files; the radius is a constant, matching the web
  component's own rule that it can never be overridden. Round icon buttons are
  a separate type, since those really are circles.
- The theme gained `surface_2` / `surface_3`, the web's `--color-surface-*`
  steps, so a control can name the step it wants instead of borrowing a token
  meant for something else.
- `Canvas::draw_shadow` (any layered CSS `box-shadow`, in any colour) and
  `Canvas::erase_rounded`, and the bold weight of the embedded font.

### Fixed

- **Deleting a folder locally never removed it from the server.** Change
  detection only ever walked the FILE index, so a folder deleted on disk had its
  contents trashed and the folder itself left behind, empty — and no later sync
  could ever catch up, because nothing re-examined it. Folders are now checked
  too and trashed through `POST /folders/:id/trash`, deepest first. Two guards
  come with it: a folder is only considered deleted when the directory that
  should CONTAIN it is itself present (a parent that is merely unavailable no
  longer takes its whole subtree down), and folders count towards the
  mass-deletion guard alongside files. Upgrading flushes the backlog: every
  folder deleted locally while this was broken is trashed on the next sync, in
  one go. They land in the drive's trash, not in a hard delete.

- **Duplicate entries in Explorer's navigation pane.** An instance could be
  registered both as a CloudFiles sync root (HKLM) and as a shell namespace
  entry (HKCU). Ownership no longer rests on the `KubunoInstanceId` marker
  alone — entries written by older builds carry none and were therefore
  invisible to pruning — and stale entries are matched by their expected CLSID,
  compared case-insensitively.
- **Unpainted (black) areas after a resize.** The window left its background
  transparent for the system backdrop to show through, and that backdrop does
  not follow the window as it grows. The client area now paints its own opaque
  background, which also matches the web shell. The composition tree is
  committed after the swap chain is resized, and a failed resize is logged
  instead of swallowed.
- **An expired access token emptied the launcher.** The module list is fetched
  through the engine (`modules_for`), which refreshes and rotates the token,
  instead of reading the stored one — the same flaw the Tauri build had, hidden
  there by the background loop keeping the token fresh.
- The waffle no longer draws an outline around itself. The web's `border-border`
  resolves to a 0.6px hairline over an opaque panel; stroked at a whole DIP it
  came out nearly three times heavier and read as a drawn ring. The drop shadow
  defines the edge instead.
- A hovered row of the account panel was filled at the CARD's radius, so a row
  in the middle came out with four rounded corners and the first and last spilled
  past the card's own. The card now clips its rows, which is what
  `overflow-hidden` does on the web: the fill is a plain rectangle and only the
  first and last rows take the corners.
- **The waffle's scrollbar was decoration.** It was drawn from the scroll
  position but answered to nothing: the thumb now drags (tracking the pointer
  exactly, and keeping it until the button is released even if it wanders off
  the bar), the track pages towards a click, and the thumb darkens under the
  pointer as the web's rule does.
- **The waffle would not scroll.** The panel lives in its own popup window, and
  "scroll inactive windows when I hover over them" — on by default since
  Windows 10 — routes the wheel to the window under the CURSOR rather than to
  the focused one. That window is the popup, which owns no scroll state, so
  every notch fell on the floor. The popup now forwards wheel messages to its
  owner. Mouse capture does not help here: the routing is settled before the
  message is posted, which is also why sending the wheel straight to the main
  window appeared to work.
- **The page reacted to the pointer THROUGH the open waffle.** The panel's hit
  test returned the same answer for "outside the panel" and "inside it, but on
  a gap", so hovering the panel's own background fell through to the header,
  the rail and the page underneath — and a click there closed the menu, which
  the web never does. The two are now distinct answers, and moving onto the
  panel clears whatever was left lit beneath it.
- **The waffle's margins did not match the web's.** Its panel reserves a
  scrollbar gutter on BOTH edges (`scrollbar-gutter: stable both-edges` over an
  8px scrollbar), which nothing in the class names hints at: every margin
  inside is measured from 8.6 in, not from the panel edge. Missing it made the
  content 16 wider and the tiles 104 instead of 98. The grid had also lost its
  `gap-1` between tiles, the card header used 24 where the web uses `px-5`, and
  the tile label sat 8 too low because it was centred in the leftover space
  rather than placed on its own line. All of it is now pinned by a test against
  browser-measured values, and the panel draws the reserved gutter's thumb.
- The waffle's title is bold, as the web's `font-semibold` actually renders:
  its font stack has no 600 face, and CSS matching resolves upwards to bold.
- A missing `WM_MOUSELEAVE` import made the constant an irrefutable binding,
  silently killing every later match arm: clicks, window close, tray messages
  and sync completion never ran.

[Unreleased]: https://github.com/kubuno/desktop/compare/v0.1.0-alpha...HEAD
