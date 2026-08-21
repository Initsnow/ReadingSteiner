use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::control::{self, ControlRequest, ControlResponse};
use crate::error::{Error, Result};
use crate::scheduler::{self, AppState};

#[derive(Debug, Parser)]
#[command(
    name = "reading-steiner",
    bin_name = "reading-steiner",
    version,
    about = "ReadingSteiner - web/data change detection with Telegram push"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the daemon in the foreground (starts control socket + web console)
    Serve {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Print the Web console address
    Web {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Show daemon status
    Status {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Add monitoring sources from a YAML file (requires running daemon)
    Sources {
        #[command(subcommand)]
        action: SourcesAction,
    },
    /// Run a check for a source immediately
    Check {
        id: String,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Show a change event diff
    Diff {
        event_id: i64,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Send a test Telegram notification
    NotifyTest {
        #[arg(long)]
        chat_id: Option<String>,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Show change history for a source
    History {
        id: Option<String>,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Show current global settings
    Settings {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Create a full backup (db + media + config)
    Backup {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// List available backups
    Backups {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Restore from a backup by name (requires daemon stopped)
    Restore {
        name: String,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum SourcesAction {
    Add {
        file: PathBuf,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    List {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Serve { config } => {
            init_tracing("info");
            let cfg = Config::load(&config)?;
            let state = Arc::new(AppState::with_config_path(cfg, Some(config.clone()))?);
            let control_state = state.clone();
            let control = tokio::spawn(async move {
                if let Err(e) = control::serve_control(control_state).await {
                    eprintln!("control server error: {e}");
                }
            });
            let web_state = state.clone();
            let web = tokio::spawn(async move {
                if let Err(e) = crate::web::serve_web(web_state).await {
                    eprintln!("web server error: {e}");
                }
            });
            scheduler::run_daemon(state).await?;
            control.abort();
            web.abort();
            Ok(())
        }
        Command::Web { config } => {
            let cfg = Config::load(&config)?;
            let addr = cfg.web.effective_listen();
            println!("ReadingSteiner web console: http://{addr}");
            println!("  static dir: {}", cfg.web.static_dir().display());
            println!(
                "  hint: start daemon with `reading-steiner serve --config {}` to serve it",
                config.display()
            );
            Ok(())
        }
        Command::Status { json, config } => {
            let cfg = Config::load(&config)?;
            let resp = control::send_request(cfg.socket_path(), &ControlRequest::Status).await?;
            print_response(&resp, json);
            Ok(())
        }
        Command::Sources { action } => match action {
            SourcesAction::Add { file, config } => {
                let cfg = Config::load(&config)?;
                let text = std::fs::read_to_string(&file)?;
                let sources: Vec<crate::config::SourceConfig> =
                    if text.trim_start().starts_with('-') {
                        serde_yaml::from_str(&text)?
                    } else {
                        vec![serde_yaml::from_str(&text)?]
                    };
                for source in sources {
                    let resp = control::send_request(
                        cfg.socket_path(),
                        &ControlRequest::SourcesAdd {
                            source: Box::new(source),
                        },
                    )
                    .await?;
                    if !resp.ok {
                        return Err(Error::other(resp.error.unwrap_or_default()));
                    }
                    let id = resp
                        .result
                        .as_ref()
                        .and_then(|r| r.get("source_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    println!("added source: {id}");
                }
                Ok(())
            }
            SourcesAction::List { config } => {
                let cfg = Config::load(&config)?;
                let resp =
                    control::send_request(cfg.socket_path(), &ControlRequest::ListSources).await?;
                if !resp.ok {
                    return Err(Error::other(resp.error.unwrap_or_default()));
                }
                let sources: Vec<crate::config::SourceConfig> =
                    serde_json::from_value(resp.result.unwrap_or_default())?;
                for s in &sources {
                    println!(
                        "{:<24} {} {}",
                        s.id,
                        if s.enabled { "enabled" } else { "disabled" },
                        s.fetch.url
                    );
                }
                Ok(())
            }
        },
        Command::Check { id, config } => {
            let cfg = Config::load(&config)?;
            let resp =
                control::send_request(cfg.socket_path(), &ControlRequest::Check { source_id: id })
                    .await?;
            print_response(&resp, false);
            Ok(())
        }
        Command::Diff { event_id, config } => {
            let cfg = Config::load(&config)?;
            let resp = control::send_request(cfg.socket_path(), &ControlRequest::Diff { event_id })
                .await?;
            print_response(&resp, false);
            Ok(())
        }
        Command::NotifyTest { chat_id, config } => {
            let cfg = Config::load(&config)?;
            let resp =
                control::send_request(cfg.socket_path(), &ControlRequest::NotifyTest { chat_id })
                    .await?;
            print_response(&resp, false);
            Ok(())
        }
        Command::History { id, config } => {
            let cfg = Config::load(&config)?;
            let resp = control::send_request(
                cfg.socket_path(),
                &ControlRequest::History {
                    source_id: id,
                    limit: Some(20),
                },
            )
            .await?;
            print_response(&resp, false);
            Ok(())
        }
        Command::Settings { config } => {
            let cfg = Config::load(&config)?;
            let resp =
                control::send_request(cfg.socket_path(), &ControlRequest::GetSettings).await?;
            print_response(&resp, false);
            Ok(())
        }
        Command::Backup { config } => {
            let cfg = Config::load(&config)?;
            // 优先通过 daemon 在线备份（一致性快照）；daemon 不可用时回退到直接读库。
            let resp = control::send_request(cfg.socket_path(), &ControlRequest::Backup).await;
            match resp {
                Ok(r) if r.ok => {
                    print_response(&r, false);
                    Ok(())
                }
                _ => {
                    // daemon 未运行：直接打开数据库做备份。
                    match crate::backup::backup_from_path(&cfg, Some(&config)) {
                        Ok(dir) => {
                            println!("backup created: {}", dir.display());
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
            }
        }
        Command::Backups { config } => {
            let cfg = Config::load(&config)?;
            match crate::backup::list_backups(&cfg.state_dir) {
                Ok(names) => {
                    if names.is_empty() {
                        println!("no backups found");
                    } else {
                        for n in &names {
                            println!("{n}");
                        }
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Command::Restore { name, config } => {
            let cfg = Config::load(&config)?;
            // 恢复会覆盖当前数据库与 media，需 daemon 停止。
            let dir = cfg.state_dir.join("backups").join(&name);
            if !dir.exists() {
                return Err(Error::other(format!("backup {name} not found")));
            }
            println!("restoring from backup {name} ...");
            crate::backup::restore(&dir, &cfg)?;
            println!("restore complete. restart daemon to load restored data.");
            Ok(())
        }
    }
}

fn print_response(resp: &ControlResponse, as_json: bool) {
    if as_json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap_or_default());
    } else if resp.ok {
        if let Some(result) = &resp.result {
            println!(
                "{}",
                serde_json::to_string_pretty(result).unwrap_or_default()
            );
        } else {
            println!("ok");
        }
    } else {
        eprintln!("error: {}", resp.error.as_deref().unwrap_or("unknown"));
        std::process::exit(1);
    }
}

fn init_tracing(level: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .init();
}
