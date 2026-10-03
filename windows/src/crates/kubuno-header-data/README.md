# kubuno-header-data

The data of a Kubuno header for the desktop apps that are **not** the shell (Documents, Chat, and later Drive…):
the `LauncherService` and `AccountService` that `kubuno-shell-controls`' `WaffleButton`, `AccountButton` and
`HeaderActions` read, fetched as the shell's current account through the shell's token broker, off the UI
thread, and kept on disk so a header looks the same offline.

The shell feeds the same controls from its own state (`services::favorites::ShellLauncher`, `ShellAccounts`):
it owns the accounts. Every other app borrows them.

## Why a crate of its own, and why here

- It implements the traits of `kubuno-shell-controls` (a user-control library on the `kubuno` facade) and posts
  to the UI thread with the facade's `UiDispatcher`: it belongs to the UI workspace (`desktop/windows/src/crates`),
  not to `desktop/common`, whose crates never link the UI stack.
- Its network and disk parts use the platform-neutral common crates only (`kubuno-account`: the broker, the
  paths; `kubuno-api-client`: the HTTP client) — not `kubuno-sync`, the file-sync engine, which an app does not
  need for its header.
- It depends on no app crate, and no app depends on another (module isolation, vskubuno `docs/VIEWS-SPEC.md`).
- The pure parts the shell had (`parse_modules`, `module_label`, `migrate_favorites`, `initials_of`, the tile of
  an app, the content-addressed picture cache, the web logos embedded at build time) moved here, so the shell
  and the apps read `/api/v1/modules` one way.

## How the data flow

```
UI thread                                   worker thread "kubuno-header" (Tokio, current thread)
---------                                   ----------------------------------------------------
start(options, config, sample, dispatcher)
  install(): set_default_launcher/accounts   1. last account's copy on disk -> Status::Cached
                                             2. AppBroker::for_app(config.app)   (verifies the shell, starts it
                                                with --background when it is not running)
                                                accounts() + pick_current()      -> SignedOut / NoShell
                                                GET /api/v1/modules, GET /api/v1/me as that account
                                                (AppTokenSource: borrowed access tokens only)
                                                -> kept under <data>/accounts/<key>/blobs/header/
                                                pictures: download -> PictureCache, else the copy kept,
                                                else the web's logo embedded at build time
  apply(snapshot) <- dispatcher.begin_invoke  <- sink(HeaderSnapshot)
    set_default_launcher/accounts again        3. waits: FeedCommand (Refresh, SaveFavorites, SwitchAccount),
    (the waffle reads it when it opens,           a broker event (switched, added, removed, session
     every avatar takes its look again)           expired/restored), or 15 minutes
```

Offline-first: the copies (`modules.json`, `me.json`, the pictures and their `index.json`) are shared by every
app of the account; `<cache>/header/last-account` names the account the last run showed, so a start without
the shell or the network still shows the last header. A server that does not answer shows the copies
(`Status::Offline`); a session the shell says has ended shows them too (`Status::Expired`).

## What a pick does

Pure functions (`plan_launch`, `plan_account`) decide; a performer carries out (the default one, or a stub
given with `set_performer` in tests and demos).

| Pick | Action |
|---|---|
| the running app's own tile (`HeaderOptions::for_app(&["office-documents"])`) | its window comes forward (`restore_and_focus`) |
| an app with a desktop build next to this program (`DESKTOP_APPS`: Chat → `kubuno-chat`, Drive → `drive`) | that program starts, detached, with this process's environment (a sandboxed app starts a sandboxed app; Chat's single instance raises a running window) |
| any other app (Documents included: started without a document it would open its sample) | its web route on the account's server, in the browser |
| « Gérer votre compte », the avatar's camera | `<server>/settings` on the web |
| « Étiquettes », « Administration » (administrators only, checked again) | `<server>/labels`, `<server>/admin` |
| another account | the broker's `switch_account`: the shell and every app follow (the broker's `Switched` event refreshes every header) |
| « Ajouter un compte », « Se déconnecter », « Supprimer » | the shell's job (it owns the accounts and the refresh tokens). **No channel exists yet**: the broker protocol has no "show this page" request and the shell has no single-instance hand-off (a second `kubuno-desktop --page …` would start a second shell in broker-client mode, which can neither sign in nor out). The default performer logs it. Follow-up: a broker request (`show_page {page}`) the shell's window answers with its `--page` targets (`login`, `accounts`). |

The browser is **never** opened by a sandboxed run (`KUBUNO_SANDBOX_DIR`) nor by the offline sample: the URL is
logged (`kubuno_account::paths::system_integration_allowed`).

## Sample and design data

- `sample_requested(args)`: `--sample`, or a Debug build under a debugger without `--live` (the shell's rule).
  The sample is the controls' design data (`HeaderSnapshot::sample()`: `design/apps.json`, `design/accounts.json`
  of `kubuno-shell-controls`): no broker, no network, nothing written.
- In the Visual Studio designer the buttons show their own design data (the services are not installed there).
- With no account signed in in the shell, the header says so (`Status::SignedOut`: empty launcher, no avatar);
  it does not invent one.

## Using it in an app

```toml
kubuno-shell-controls = { path = "../crates/kubuno-shell-controls" }   # registers HeaderActions
kubuno-header-data    = { path = "../crates/kubuno-header-data" }
```

```xml
<!-- On the view's root: the window places HeaderActions in its title bar, left of the caption buttons. -->
<Panel ... ShowWaffle="true" ShowAccount="true">
```

(or a `<HeaderActions TitleBar.Region="Right" Compact="true" ShowNotifications="false" ShowSettings="false"
ShowHelp="false" .../>` placed by hand).

```rust
// In the window's Load (it has a dispatcher there):
let mut config = kubuno_header_data::FeedConfig::new("kubuno-chat");
config.proxy = kubuno_sync::get_proxy();
kubuno_header_data::start(kubuno_header_data::HeaderOptions::for_app(&["chat"]), config, sample, self.dispatcher());
```

## Tests

`cargo test -p kubuno-header-data`: `/api/v1/modules` parsing (logos, roots, single-app modules, the favourites'
migration), the picture cache (content names, error pages never cached, replaced files removed), the snapshot
built from the server's answers and from the broker's accounts, the sample, the launch and account-panel plans,
and the services following the applied snapshot with a stub performer (nothing is opened).
