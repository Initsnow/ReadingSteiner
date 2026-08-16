use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tracing::info;

use crate::config::Config;
use crate::control::{self, ControlRequest, ControlResponse};
use crate::error::{Error, Result};
use crate::scheduler::{self, AppState};

#[derive(Debug, Parser)]
#[command(
    name = "wwatch",
    version,
    about = "ReadingSteiner - web/data change detection with Telegram push"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the daemon in the foreground
    Serve {
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Open the TUI
    Tui {
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
    /// Add monitoring sources from a YAML file
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
    /// Test pipeline with the latest snapshot
    TestPipeline {
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
            let state = Arc::new(AppState::new(cfg)?);
            let control_state = state.clone();
            let control = tokio::spawn(async move {
                if let Err(e) = control::serve_control(control_state).await {
                    eprintln!("control server error: {e}");
                }
            });
            scheduler::run_daemon(state).await?;
            control.abort();
            Ok(())
        }
        Command::Tui { config } => {
            let cfg = Config::load(&config)?;
            crate::tui::run_tui(&cfg).await
        }
        Command::Status { json, config } => {
            let cfg = Config::load(&config)?;
            let resp = control::send_request(cfg.socket_path(), &ControlRequest::Status).await?;
            print_response(&resp, json);
            Ok(())
        }
        Command::Sources { action } => match action {
            SourcesAction::Add { file, config } => {
                let mut cfg = Config::load(&config)?;
                let text = std::fs::read_to_string(&file)?;
                let sources: Vec<crate::config::SourceConfig> =
                    if text.trim_start().starts_with('-') {
                        serde_yaml::from_str(&text)?
                    } else {
                        vec![serde_yaml::from_str(&text)?]
                    };
                for source in sources {
                    if cfg.sources.iter().any(|s| s.id == source.id) {
                        return Err(Error::config(format!(
                            "source {} already exists",
                            source.id
                        )));
                    }
                    cfg.sources.push(source);
                }
                cfg.save(&config)?;
                info!(path = %config.display(), "sources added to config");
                println!(
                    "added {} source(s) to {}",
                    cfg.sources.len(),
                    config.display()
                );
                Ok(())
            }
            SourcesAction::List { config } => {
                let cfg = Config::load(&config)?;
                for s in &cfg.sources {
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
        Command::TestPipeline { id, config } => {
            let cfg = Config::load(&config)?;
            let resp = control::send_request(
                cfg.socket_path(),
                &ControlRequest::TestPipeline { source_id: id },
            )
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
