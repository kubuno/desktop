//! Kubuno desktop sync daemon (CLI). Thin wrapper over the `kubuno_sync` library.

use anyhow::Result;
use clap::{Parser, Subcommand};
use kubuno_sync::{daemon, list_instances, modules_for, sync_once};

#[derive(Parser)]
#[command(name = "kubuno-sync", about = "Daemon de synchronisation de fichiers Kubuno (offline-first)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ajouter un dossier synchronisé pour un compte déjà connecté dans Kubuno Desktop.
    Add {
        #[arg(long)]
        server: String,
        #[arg(long)]
        folder: String,
    },
    /// Synchroniser une fois (push local puis pull serveur).
    Sync,
    /// Synchroniser en continu (watcher de fichiers + poll serveur).
    Watch {
        /// Intervalle de poll serveur en secondes.
        #[arg(long, default_value_t = 30)]
        interval: u64,
    },
    /// Afficher l'état courant.
    Status,
    /// Dump the raw modules JSON from the server (debug).
    Modules,
    /// Déplacer le dossier local d'une instance (copie + rebase de l'état).
    Move {
        /// Identifiant de l'instance (voir `status`).
        #[arg(long)]
        id: String,
        /// Nouveau chemin du dossier de synchronisation.
        #[arg(long)]
        to: String,
    },
}

fn main() -> Result<()> {
    // Bring any legacy single-instance layout under instances/<id>/.
    kubuno_sync::migrate_legacy()?;
    // Tokens come from the Kubuno shell's broker (the shell is the only refresh-token owner; it is started in
    // the background when it is not running and lives next to this program).
    match kubuno_sync::tokens::BrokerProvider::for_app("kubuno-sync") {
        Ok(p) => kubuno_sync::tokens::install(std::sync::Arc::new(p)),
        Err(e) => eprintln!("Courtier de jetons indisponible : {e}"),
    }

    match Cli::parse().cmd {
        Cmd::Add { server, folder } => {
            // The sign-in (password, two-factor code) happens in Kubuno Desktop, which owns the tokens.
            let accounts = kubuno_sync::tokens::accounts()?;
            let norm = |s: &str| s.trim_end_matches('/').to_ascii_lowercase();
            let Some(account) = accounts.iter().find(|a| norm(&a.server_url) == norm(&server)) else {
                anyhow::bail!("aucun compte connecté sur {server} : connectez-vous d'abord dans Kubuno Desktop");
            };
            let id = kubuno_sync::register_instance(&account.server_url, &folder)?;
            println!("Dossier « {folder} » ajouté (instance « {id} ») ; Kubuno Desktop le rattache au compte au prochain démarrage.");
        }
        Cmd::Sync => {
            let instances = list_instances();
            if instances.is_empty() {
                println!("Aucune instance configurée. Connectez-vous dans Kubuno Desktop, puis `kubuno-sync add`.");
            }
            for cfg in instances {
                println!("── {} ({}) ──", cfg.id, cfg.server_url);
                match sync_once(&cfg.id) {
                    Ok(s) => {
                        println!(
                            "Envoi : {} créé(s), {} modifié(s), {} supprimé(s), {} conflit(s), {} en attente.",
                            s.uploaded, s.modified, s.deleted_up, s.conflicts, s.pending
                        );
                        println!(
                            "Réception : {} téléchargé(s), {} dossier(s), {} à jour, {} supprimé(s). Curseur : {}",
                            s.downloaded, s.folders, s.up_to_date, s.deleted_down, s.cursor
                        );
                    }
                    Err(e) => eprintln!("Échec de la synchro : {e}"),
                }
            }
        }
        Cmd::Watch { interval } => {
            daemon::watch_all(interval, |_id, _ev| {})?;
        }
        Cmd::Status => {
            let instances = list_instances();
            if instances.is_empty() {
                println!("Aucune instance configurée.");
            }
            for cfg in instances {
                let cursor = kubuno_sync::store::Store::open(&kubuno_sync::db_path(&cfg.id)?)
                    .and_then(|s| s.cursor())
                    .unwrap_or(0);
                println!("Instance : {}", cfg.id);
                println!("  Serveur : {}", cfg.server_url);
                println!("  Dossier : {}", cfg.sync_root.display());
                println!("  Curseur : {cursor}");
            }
        }
        Cmd::Modules => {
            for cfg in list_instances() {
                println!("── {} ──", cfg.id);
                match modules_for(&cfg.id) {
                    Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()),
                    Err(e) => eprintln!("échec: {e}"),
                }
            }
        }
        Cmd::Move { id, to } => {
            println!("Déplacement de l'instance « {id} » vers {to} (copie + rebase)…");
            kubuno_sync::move_instance_folder(&id, &to)?;
            println!("Terminé. Nouveau dossier : {to}");
        }
    }
    Ok(())
}
