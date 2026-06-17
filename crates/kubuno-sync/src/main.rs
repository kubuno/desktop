//! Kubuno desktop sync daemon (CLI). Thin wrapper over the `kubuno_sync` library.

use anyhow::Result;
use clap::{Parser, Subcommand};
use kubuno_sync::{daemon, sync_once, Config};

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
    match Cli::parse().cmd {
        Cmd::Login { server, login, password, folder } => {
            kubuno_sync::login(&server, &login, &password, &folder)?;
            println!("Connecté. Dossier synchronisé : {folder}");
        }
        Cmd::Sync => {
            let s = sync_once()?;
            println!(
                "Envoi : {} créé(s), {} modifié(s), {} supprimé(s), {} conflit(s), {} en attente.",
                s.uploaded, s.modified, s.deleted_up, s.conflicts, s.pending
            );
            println!(
                "Réception : {} téléchargé(s), {} dossier(s), {} à jour, {} supprimé(s). Curseur : {}",
                s.downloaded, s.folders, s.up_to_date, s.deleted_down, s.cursor
            );
        }
        Cmd::Watch { interval } => {
            daemon::watch(interval)?;
        }
        Cmd::Status => {
            let cfg = Config::load()?;
            let store = kubuno_sync::store::Store::open(&kubuno_sync::db_path()?)?;
            println!("Serveur : {}", cfg.server_url);
            println!("Dossier : {}", cfg.sync_root.display());
            println!("Curseur : {}", store.cursor()?);
        }
    }
    Ok(())
}
