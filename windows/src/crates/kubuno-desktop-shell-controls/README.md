# kubuno-desktop-shell-controls

The menus of a Kubuno header — the app launcher and the account panel every module's header shows on the web — as
user controls any Kubuno desktop app can place in its views. Names, properties, events and data shapes are shared
with the web: see vskubuno `docs/SHELL-CONTROLS.md` (the contract).

| Class | Kind | What |
|---|---|---|
| `WaffleMenu` | user control (`controls/waffle_menu.kbcontrol`) | The app launcher: the favourites card (title and pencil; while editing « Annuler », « OK » and the help line) over every other app, scrolling as one. Properties `Apps`, `Favorites`, `Editing`; events `AppLaunched`, `FavoritesEdited`, `EditModeChanged`, `ContentHeightChanged`, `CloseRequested`. |
| `AccountMenu` | user control (`controls/account_menu.kbcontrol`) | The account panel: address and close button, avatar, greeting, « Gérer votre compte », the other accounts, the actions. Properties `User`, `Accounts`, `ShowAdmin`; events `ManageAccount`, `OpenAccount`, `RemoveAccount`, `AddAccount`, `OpenLabels`, `OpenAdmin`, `SignOut`, `ChangeAvatar`, `CloseRequested`. |
| `WaffleButton`, `AccountButton` | user controls | The header's waffle and avatar: a click opens the menu in a popup of its own (`kubuno_desktop::popup::Popup`), which may extend beyond the app's window and stays on the screen. |
| `HeaderActions` | user control | The web's right-hand header cluster: bell (+ counter), settings, help, waffle, avatar; `Show…` properties, `Minimal`, `UnreadCount`; events `NotificationsClicked`, `SettingsClicked`, `HelpClicked`. |
| `AppTileGrid`, `PanelMenu`, `AccentPill` | custom controls | The parts the menus draw with (tiles and favourites edit; a card of rows; the outlined pill). |

## Why this crate

It sits on top of the `kubuno-desktop` facade (it is made of `.kbcontrol` user controls with their own resources and
code-behind), so it cannot live in `kubuno-desktop-controls` or `kubuno-desktop-views`, which the facade itself depends on. It
depends on no app crate, and no app crate depends on another: reuse goes through shared crates (vskubuno
`docs/VIEWS-SPEC.md`, "Module isolation"). The view macros, the views language server and the Visual Studio designer
find its controls like any control library's: only the framework's own crates (an explicit list,
`kubuno-desktop-views-meta` `FRAMEWORK_CRATES`) are skipped, whatever the prefix of a library's name.

## Using it in an app

1. Add the dependency: `kubuno-desktop-shell-controls = { path = "../crates/kubuno-desktop-shell-controls" }` (or `workspace = true`).
   The controls then appear in the Visual Studio Toolbox (« <project> Composants ») after a build, and render in the
   designer with their sample data (`controls/design/apps.json`: twelve apps; `controls/design/accounts.json`: three
   accounts).
2. Give the controls their data once, at start-up and whenever it changes:

   ```rust
   kubuno_desktop_shell_controls::set_default_launcher(Rc::new(MyLauncher));   // impl LauncherService
   kubuno_desktop_shell_controls::set_default_accounts(Rc::new(MyAccounts));   // impl AccountService
   ```

   or per control: `self.waffle.with(|b| b.set_service(rc))`.
3. Drop `<HeaderActions TitleBar.Region="Right" Width="182" Height="64"/>` in the window's title bar (or a
   `WaffleButton` / `AccountButton` alone), and handle `NotificationsClicked`, `SettingsClicked`, `HelpClicked`.

`examples/header_demo.rs` is a 320 × 200 window with a `HeaderActions`: its menus open outside it
(`cargo run -p kubuno-desktop-shell-controls --example header_demo [-- --dark]`).

### Where the data come from

- **The shell** implements both services from its own state: `services::favorites::ShellLauncher` (the instance's
  modules from `/api/v1/modules`, the account's `preferences.waffle_favorites`, a launch opens the app's route in the
  browser, a saved list goes back to the server) and `ShellAccounts` in `views/shell_window.rs` (the signed-in user,
  the other configured instances, the console's visibility; picks become the window's actions).
- **Another app** (Documents, Chat; Drive later) uses the shared crate `kubuno-desktop-header-data`
  (`src/crates/kubuno-desktop-header-data`): one call in the window's `Load` (`kubuno_desktop_header_data::start`) installs both
  services on the UI thread and starts a worker that, as the shell's current account (the token broker of
  `desktop/common/kubuno-desktop-account`, borrowed access tokens only), fetches `/api/v1/modules` and `/api/v1/me` through
  `kubuno-desktop-api-client`, keeps them and the pictures under `<data>/accounts/<key>/blobs/header/` (offline-first),
  follows the broker's events (account switched, session expired…) and calls `set_default_launcher` /
  `set_default_accounts` again with each new snapshot. A tile opens the app itself (its window comes forward),
  another desktop app installed next to it (Chat, Drive), or the web route in the browser (never in a sandboxed
  run); another account is switched to through the broker; « Ajouter un compte » / « Se déconnecter » are the
  shell's (no channel to ask it yet: logged). `--sample` (and a Debug run under a debugger) shows the design data.
  The parsing of `/api/v1/modules`, the favourites' migration and the logo cache moved into that crate (see its
  README).

## Strings

`resources/shell_controls.kbres` (+ `shell_controls.fr.kbres`), class `ShellControlsResources`. The views read them
with an explicit set — `{Res launcher_title, Source=shell_controls}` — because a `{Res key}` lookup searches every
registered set, and an app may have a key of the same name (the shell's `launcher_title` is its home page's title).

## Tests

`cargo test -p kubuno-desktop-shell-controls`: the launcher's geometry (measured on the web panel), the favourites edit (toggle,
drag, unknown ids kept), the menus' view models and events, the views compiling, the popup's anchor rules.
