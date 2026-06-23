//! Kubuno desktop sync daemon (CLI). Thin wrapper over the `kubuno_sync` library.

use anyhow::Result;
use clap::{Parser, Subcommand};
use kubuno_sync::{daemon, list_instances, sync_once};

#[derive(Parser)]
#[command(name = "kubuno-sync", about = "Daemon de synchronisation de fichiers Kubuno (offline-first)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Se connecter et configurer le dossier synchronisé.
    Login {
        #[arg(long)]
        server: String,
        #[arg(long)]
        login: String,
        #[arg(long)]
        password: String,
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
}

fn main() -> Result<()> {
    // Bring any legacy single-instance layout under instances/<id>/.
    kubuno_sync::migrate_legacy()?;

    match Cli::parse().cmd {
        Cmd::Login { server, login, password, folder } => {
            let id = kubuno_sync::login(&server, &login, &password, &folder)?;
            println!("Connecté (instance « {id} »). Dossier synchronisé : {folder}");
        }
        Cmd::Sync => {
            let instances = list_instances();
            if instances.is_empty() {
                println!("Aucune instance configurée. Lance `kubuno-sync login` d'abord.");
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
    }
    Ok(())
}
