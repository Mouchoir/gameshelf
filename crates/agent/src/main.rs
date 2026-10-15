//! `gameshelf-agent` command line.

use clap::{Parser, Subcommand};
use gameshelf_agent::{
    autostart,
    client::parse_server,
    config,
    messages::tr,
    scan,
    service::{Agent, CycleError, CycleReport, hostname},
};
use std::io::Write;

#[derive(Parser)]
#[command(
    name = "gameshelf-agent",
    version,
    about = "Reports the games of this computer's launchers to a gameshelf server."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Guided setup: shows what would be sent, then asks for the server, the pairing code and autostart.
    Setup {
        /// Server address, to skip that question.
        server: Option<String>,
    },
    /// Pairs with a server using a code from its Agents page.
    Pair {
        server: String,
        code: String,
        /// Name shown in the web app (default: this computer's name).
        #[arg(long)]
        name: Option<String>,
    },
    /// Shows what the agent would send, without sending anything.
    Scan,
    /// Runs in the background: pulls settings, scans on schedule and sends reports.
    Run {
        /// One cycle, scanning now, then exit.
        #[arg(long)]
        once: bool,
    },
    /// Shows the pairing, settings and sources.
    Status,
    /// Stops sending anything from this computer.
    Pause,
    /// Sends again after a pause.
    Resume,
    /// Turns a source on or off on this computer.
    Source {
        /// steam-local, epic-local, gog-galaxy-local, battlenet-local, ubisoft-local or launchers-local.
        id: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Starts the agent with this computer, or stops doing so.
    Autostart {
        #[arg(value_parser = ["enable", "disable", "status"])]
        action: String,
    },
    /// Removes this agent from the server and forgets the pairing.
    Unpair,
}

fn ask(question: &str) -> Result<String, String> {
    print!("{question}");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim().to_string())
}

fn ask_yes(question: &str, default: bool) -> Result<bool, String> {
    let answer = ask(question)?.to_lowercase();
    Ok(match answer.as_str() {
        "" => default,
        a => a.starts_with('y') || a.starts_with('o'),
    })
}

fn print_cycle(r: &CycleReport) {
    if r.offline {
        println!(
            "{}",
            tr(
                "The server cannot be reached; nothing was sent.",
                "Le serveur est injoignable ; rien n'a été envoyé."
            )
        );
    }
    if r.paused {
        println!(
            "{}",
            tr("Paused: nothing was sent.", "En pause : rien n'a été envoyé.")
        );
    }
    for (source, platform, result) in &r.sent {
        let what = if result == "unchanged" {
            tr("unchanged", "inchangé")
        } else {
            tr("sent", "envoyé")
        };
        println!("  {source} -> {platform}: {what}");
    }
    for (source, platform, e) in &r.failed {
        println!(
            "  {source} -> {platform}: {} {e}",
            tr("not sent:", "non envoyé :")
        );
    }
    for (source, e) in &r.source_errors {
        println!("  {source}: {} {e}", tr("cannot read:", "lecture impossible :"));
    }
}

fn explain(e: &CycleError) -> String {
    match e {
        CycleError::NotPaired => tr(
            "This agent is not paired. Run `gameshelf-agent setup`.",
            "Cet agent n'est pas appairé. Lancez `gameshelf-agent setup`.",
        )
        .to_string(),
        CycleError::Revoked => tr(
            "This agent was removed in the web app. Its pairing was forgotten; run `gameshelf-agent setup` to pair it again.",
            "Cet agent a été retiré dans l'application web. Son appairage a été oublié ; lancez `gameshelf-agent setup` pour l'appairer de nouveau.",
        )
        .to_string(),
        CycleError::Upgrade(v) => format!(
            "{} {v}",
            tr("Update the agent: the server needs version", "Mettez l'agent à jour : le serveur demande la version")
        ),
        CycleError::Other(m) => m.clone(),
    }
}

async fn setup(agent: &Agent, server: Option<String>) -> Result<(), String> {
    println!(
        "{}\n",
        tr(
            "gameshelf agent setup. Here is what this computer would send (nothing is sent yet):",
            "Installation de l'agent gameshelf. Voici ce que cet ordinateur enverrait (rien n'est encore envoyé) :",
        )
    );
    let cfg = agent.load()?;
    let preview = scan::run(agent.sources(), &cfg.effective_sources(), false).await;
    println!("{}\n", scan::summary(&preview));
    println!(
        "{}\n",
        tr(
            "Only game lists, playtime and install sizes are sent; never passwords, login names or e-mail addresses.",
            "Seuls la liste des jeux, le temps de jeu et la taille des installations sont envoyés ; jamais de mot de passe, d'identifiant ni d'adresse e-mail.",
        )
    );
    let server = match server {
        Some(s) => s,
        None => ask(tr(
            "1/3 Server address (for example https://games.example.com): ",
            "1/3 Adresse du serveur (par exemple https://games.example.com) : ",
        ))?,
    };
    let (server, insecure) = parse_server(&server)?;
    if insecure {
        println!(
            "{}",
            tr(
                "Warning: plain HTTP. Use it only on your home network.",
                "Attention : HTTP non chiffré. À n'utiliser que sur votre réseau local.",
            )
        );
    }
    let code = ask(tr(
        "2/3 Pairing code (Agents page of the web app): ",
        "2/3 Code d'appairage (page Agents de l'application web) : ",
    ))?;
    let name = hostname();
    agent.pair(&server, &code, &name).await?;
    println!("{} {name}", tr("Paired as", "Appairé sous le nom"));
    if ask_yes(
        tr(
            "3/3 Start the agent with this computer? [Y/n] ",
            "3/3 Démarrer l'agent avec cet ordinateur ? [O/n] ",
        ),
        true,
    )? {
        match autostart::enable() {
            Ok(()) => println!("{}", tr("Autostart on.", "Démarrage automatique activé.")),
            Err(e) => println!(
                "{} {e}",
                tr(
                    "Autostart could not be set:",
                    "Impossible d'activer le démarrage automatique :"
                )
            ),
        }
    }
    println!("{}", tr("First scan...", "Première analyse..."));
    match agent.cycle(true).await {
        Ok(r) => print_cycle(&r),
        Err(e) => println!("{}", explain(&e)),
    }
    println!(
        "\n{}",
        tr(
            "Done. The agent scans every 6 hours; change that, pause it or turn sources off on the Agents page, or here with `gameshelf-agent pause`, `source` and `unpair`.",
            "Terminé. L'agent analyse toutes les 6 heures ; modifiez cela, mettez-le en pause ou coupez des sources depuis la page Agents, ou ici avec `gameshelf-agent pause`, `source` et `unpair`.",
        )
    );
    Ok(())
}

fn status(agent: &Agent) -> Result<(), String> {
    let cfg = agent.load()?;
    let server = cfg.server();
    match (&cfg.server_url, cfg.paired()) {
        (Some(url), true) => println!("{} {url}", tr("Server:", "Serveur :")),
        _ => println!("{}", tr("Not paired.", "Non appairé.")),
    }
    if let Some(name) = &cfg.name {
        println!("{} {name}", tr("Name:", "Nom :"));
    }
    let paused = if cfg.paused {
        tr("paused on this computer", "en pause sur cet ordinateur")
    } else if server.status == "paused" {
        tr("paused in the web app", "en pause dans l'application web")
    } else {
        tr("active", "actif")
    };
    println!("{} {paused}", tr("State:", "État :"));
    println!(
        "{} {} h",
        tr("Scan every:", "Analyse toutes les :"),
        server.scan_hours
    );
    let when = |t: Option<time::OffsetDateTime>| {
        t.map(|t| t.to_string())
            .unwrap_or_else(|| tr("never", "jamais").to_string())
    };
    println!(
        "{} {}",
        tr("Last scan:", "Dernière analyse :"),
        when(cfg.last_scan_at)
    );
    println!(
        "{} {}",
        tr("Last contact:", "Dernier contact :"),
        when(cfg.last_heartbeat_at)
    );
    println!(
        "{} {}",
        tr("Autostart:", "Démarrage automatique :"),
        if autostart::is_enabled() {
            tr("on", "activé")
        } else {
            tr("off", "désactivé")
        }
    );
    println!(
        "{} {}",
        tr("Install folders sent:", "Dossiers d'installation envoyés :"),
        if server.send_install_paths {
            tr("yes", "oui")
        } else {
            tr("no", "non")
        }
    );
    println!("{}", tr("Sources:", "Sources :"));
    for (id, s) in scan::detect(agent.sources(), &cfg.effective_sources()) {
        let state = match (s.enabled, s.detected) {
            (false, _) => tr("off", "désactivée"),
            (true, false) => tr("on, not found here", "activée, introuvable ici"),
            (true, true) => tr("on, found", "activée, trouvée"),
        };
        let local = if cfg.disabled_sources.contains(&id) {
            tr(" (turned off on this computer)", " (coupée sur cet ordinateur)")
        } else {
            ""
        };
        println!("  {id}: {state}{local}");
    }
    Ok(())
}

fn set_paused(agent: &Agent, paused: bool) -> Result<(), String> {
    let mut cfg = agent.load()?;
    cfg.paused = paused;
    agent.save(&cfg)?;
    if paused {
        println!(
            "{}",
            tr(
                "Paused: nothing is sent until `gameshelf-agent resume`.",
                "En pause : rien n'est envoyé jusqu'à `gameshelf-agent resume`.",
            )
        );
    } else {
        println!("{}", tr("Resumed.", "Reprise."));
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("GAMESHELF_AGENT_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let agent = Agent::new(config::dir(), gameshelf_local::builtin());
    let result: Result<(), String> = match cli.command.unwrap_or(Command::Setup { server: None }) {
        Command::Setup { server } => setup(&agent, server).await,
        Command::Pair { server, code, name } => {
            let name = name.unwrap_or_else(hostname);
            agent
                .pair(&server, &code, &name)
                .await
                .map(|_| println!("{} {name}", tr("Paired as", "Appairé sous le nom")))
        }
        Command::Scan => match agent.load() {
            Ok(cfg) => {
                let s = scan::run(agent.sources(), &cfg.effective_sources(), cfg.server().send_install_paths).await;
                println!("{}", tr("What would be sent (nothing was sent):", "Ce qui serait envoyé (rien n'a été envoyé) :"));
                println!("{}", scan::summary(&s));
                Ok(())
            }
            Err(e) => Err(e),
        },
        Command::Run { once: true } => agent.cycle(true).await.map(|r| print_cycle(&r)).map_err(|e| explain(&e)),
        Command::Run { once: false } => agent.run_forever().await.map_err(|e| explain(&e)),
        Command::Status => status(&agent),
        Command::Pause => set_paused(&agent, true),
        Command::Resume => set_paused(&agent, false),
        Command::Source { id, state } => agent.load().and_then(|mut cfg| {
            if !gameshelf_core::agent::SOURCES.iter().any(|(s, _)| *s == id) {
                return Err(format!("{} {id}", tr("Unknown source:", "Source inconnue :")));
            }
            if state == "on" {
                cfg.disabled_sources.remove(&id);
            } else {
                cfg.disabled_sources.insert(id.clone());
            }
            agent.save(&cfg).map(|()| println!("{id}: {state}"))
        }),
        Command::Autostart { action } => match action.as_str() {
            "enable" => autostart::enable().map(|()| println!("{}", tr("Autostart on.", "Démarrage automatique activé."))),
            "disable" => autostart::disable().map(|()| println!("{}", tr("Autostart off.", "Démarrage automatique désactivé."))),
            _ => {
                println!("{}", if autostart::is_enabled() { tr("on", "activé") } else { tr("off", "désactivé") });
                Ok(())
            }
        },
        Command::Unpair => agent.unpair().await.map(|reached| {
            if reached {
                println!("{}", tr("Removed from the server and forgotten.", "Retiré du serveur et oublié."));
            } else {
                println!(
                    "{}",
                    tr(
                        "Forgotten here. The server could not be reached: remove the agent on its Agents page.",
                        "Oublié ici. Le serveur est injoignable : retirez l'agent depuis sa page Agents.",
                    )
                );
            }
        }),
    };
    if let Err(e) = result {
        eprintln!("gameshelf-agent: {e}");
        std::process::exit(1);
    }
}
