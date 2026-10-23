use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde_json::json;

use crate::config::Config;
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
    /// Run the daemon in the foreground (starts web console & scheduler)
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
    /// Add or list monitoring sources
    Sources {
        #[command(subcommand)]
        action: SourcesAction,
    },
    /// Run a check for a source immediately
    Check {
        id: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Show a change event diff
    Diff {
        event_id: i64,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Send a test Telegram notification
    NotifyTest {
        #[arg(long)]
        chat_id: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Show change history for a source
    History {
        id: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Show current global settings
    Settings {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Create a full backup (db + media + config)
    Backup {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// List available backups
    Backups {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Restore from a backup by name (daemon running -> online restore)
    Restore {
        name: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Delete a backup by name
    BackupDelete {
        name: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
    /// Restore from an uploaded zip backup (daemon running -> online restore)
    RestoreFromZip {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        json: bool,
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
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "config.yaml")]
        config: PathBuf,
    },
}

pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
    auth_token: String,
}

impl ApiClient {
    pub fn new(cfg: &Config) -> Self {
        let addr = cfg.web.effective_listen();
        let host_port = if addr.starts_with("0.0.0.0:") {
            format!("127.0.0.1:{}", addr.strip_prefix("0.0.0.0:").unwrap())
        } else {
            addr
        };
        let base_url = format!("http://{host_port}/api");
        let auth_token = cfg.web.auth_token.clone();
        Self {
            client: reqwest::Client::new(),
            base_url,
            auth_token,
        }
    }

    pub async fn get(&self, path: &str) -> Result<serde_json::Value> {
        self.request(reqwest::Method::GET, path, None).await
    }

    pub async fn post<T: serde::Serialize>(
        &self,
        path: &str,
        body: Option<&T>,
    ) -> Result<serde_json::Value> {
        let body_val = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| Error::other(e.to_string()))?;
        self.request(reqwest::Method::POST, path, body_val.as_ref())
            .await
    }

    pub async fn delete(&self, path: &str) -> Result<serde_json::Value> {
        self.request(reqwest::Method::DELETE, path, None).await
    }

    pub async fn upload_zip(
        &self,
        path: &str,
        file_path: &std::path::Path,
    ) -> Result<serde_json::Value> {
        let bytes = tokio::fs::read(file_path).await?;
        let file_part = reqwest::multipart::Part::bytes(bytes)
            .file_name("backup.zip")
            .mime_str("application/zip")
            .map_err(|e| Error::other(e.to_string()))?;
        let form = reqwest::multipart::Form::new().part("file", file_part);

        let url = format!("{}{path}", self.base_url);
        let mut req = self.client.post(&url).multipart(form);
        if !self.auth_token.trim().is_empty() {
            req = req.bearer_auth(self.auth_token.trim());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::other(format!("HTTP upload failed: {e}")))?;
        let status = resp.status();
        let envelope: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::other(format!("Failed to parse response JSON: {e}")))?;
        let ok = envelope
            .get("ok")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if ok {
            Ok(envelope
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null))
        } else {
            let err = envelope
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("API returned error");
            Err(Error::other(format!("{err} (status {status})")))
        }
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}{path}", self.base_url);
        let mut req = self.client.request(method, &url);
        if !self.auth_token.trim().is_empty() {
            req = req.bearer_auth(self.auth_token.trim());
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::other(format!("Daemon unreachable at {url}: {e}")))?;
        let status = resp.status();
        let envelope: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::other(format!("Failed to parse response JSON: {e}")))?;
        let ok = envelope
            .get("ok")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if ok {
            Ok(envelope
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null))
        } else {
            let err = envelope
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("API returned error");
            Err(Error::other(format!("{err} (status {status})")))
        }
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Serve { config } => {
            init_tracing("info");
            let cfg = Config::load(&config)?;
            let state = Arc::new(AppState::with_config_path(cfg, Some(config.clone()))?);
            let web_state = state.clone();
            let web = tokio::spawn(async move {
                if let Err(e) = crate::web::serve_web(web_state).await {
                    eprintln!("web server error: {e}");
                }
            });
            scheduler::run_daemon(state).await?;
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
            let client = ApiClient::new(&cfg);
            let res = client.get("/status").await?;
            print_output(&res, json);
            Ok(())
        }
        Command::Sources { action } => match action {
            SourcesAction::Add { file, config } => {
                let cfg = Config::load(&config)?;
                let client = ApiClient::new(&cfg);
                let text = std::fs::read_to_string(&file)?;
                let sources: Vec<crate::config::SourceConfig> =
                    if text.trim_start().starts_with('-') {
                        serde_yaml::from_str(&text)?
                    } else {
                        vec![serde_yaml::from_str(&text)?]
                    };
                for source in sources {
                    let res = client.post("/sources", Some(&source)).await?;
                    let id = res.get("source_id").and_then(|v| v.as_str()).unwrap_or("");
                    println!("added source: {id}");
                }
                Ok(())
            }
            SourcesAction::List { json, config } => {
                let cfg = Config::load(&config)?;
                let client = ApiClient::new(&cfg);
                let res = client.get("/sources").await?;
                if json {
                    print_output(&res, true);
                } else if let Some(arr) = res.as_array() {
                    println!("{:<24} {:<12} {:<10} URL", "ID", "GROUP", "STATUS");
                    println!("{}", "-".repeat(70));
                    for s in arr {
                        let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("-");
                        let group = s.get("group_id").and_then(|v| v.as_str()).unwrap_or("-");
                        let enabled = s.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                        let url = s.get("url").and_then(|v| v.as_str()).unwrap_or("-");
                        println!(
                            "{:<24} {:<12} {:<10} {}",
                            id,
                            group,
                            if enabled { "enabled" } else { "disabled" },
                            url
                        );
                    }
                } else {
                    print_output(&res, false);
                }
                Ok(())
            }
        },
        Command::Check { id, json, config } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            let res = client
                .post(&format!("/sources/{id}/check"), None::<&()>)
                .await?;
            print_output(&res, json);
            Ok(())
        }
        Command::Diff {
            event_id,
            json,
            config,
        } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            let res = client.get(&format!("/events/{event_id}")).await?;
            print_output(&res, json);
            Ok(())
        }
        Command::NotifyTest {
            chat_id,
            json,
            config,
        } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            let body = json!({ "chat_id": chat_id });
            let res = client.post("/notify-test", Some(&body)).await?;
            print_output(&res, json);
            Ok(())
        }
        Command::History { id, json, config } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            let path = match id {
                Some(ref s) => format!("/history?source_id={s}&limit=20"),
                None => "/history?limit=20".to_string(),
            };
            let res = client.get(&path).await?;
            print_output(&res, json);
            Ok(())
        }
        Command::Settings { json, config } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            let res = client.get("/settings").await?;
            print_output(&res, json);
            Ok(())
        }
        Command::Backup { json, config } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            match client.post("/backup", None::<&()>).await {
                Ok(res) => {
                    print_output(&res, json);
                    Ok(())
                }
                Err(_) => {
                    // daemon not running: offline fallback
                    let dir = crate::backup::backup_from_path(&cfg, Some(&config))?;
                    if json {
                        println!("{}", json!({ "backup_dir": dir.display().to_string() }));
                    } else {
                        println!("backup created offline: {}", dir.display());
                    }
                    Ok(())
                }
            }
        }
        Command::Backups { json, config } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            match client.get("/backups").await {
                Ok(res) => {
                    if json {
                        print_output(&res, true);
                    } else if let Some(backups) = res.get("backups").and_then(|v| v.as_array()) {
                        if backups.is_empty() {
                            println!("no backups found");
                        } else {
                            for b in backups {
                                let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let has_zip =
                                    b.get("has_zip").and_then(|v| v.as_bool()).unwrap_or(false);
                                println!(
                                    "{}\t{}",
                                    name,
                                    if has_zip { "(zip available)" } else { "" }
                                );
                            }
                        }
                    } else {
                        print_output(&res, false);
                    }
                    Ok(())
                }
                Err(_) => {
                    let infos = crate::backup::list_backups(&cfg.state_dir)?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&infos).unwrap_or_default()
                        );
                    } else if infos.is_empty() {
                        println!("no backups found");
                    } else {
                        for b in &infos {
                            println!(
                                "{}\t{}",
                                b.name,
                                if b.has_zip { "(zip available)" } else { "" }
                            );
                        }
                    }
                    Ok(())
                }
            }
        }
        Command::Restore { name, json, config } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            let body = json!({ "name": name });
            match client.post("/restore", Some(&body)).await {
                Ok(res) => {
                    print_output(&res, json);
                    Ok(())
                }
                Err(_) => {
                    let dir = cfg.state_dir.join("backups").join(&name);
                    if !dir.exists() {
                        return Err(Error::other(format!("backup {name} not found")));
                    }
                    println!("daemon unreachable, restoring offline ...");
                    crate::backup::restore(&dir, &cfg, None)?;
                    println!("restore complete. restart daemon to load restored data.");
                    Ok(())
                }
            }
        }
        Command::BackupDelete { name, json, config } => {
            let cfg = Config::load(&config)?;
            let client = ApiClient::new(&cfg);
            match client.delete(&format!("/backups/{name}")).await {
                Ok(res) => {
                    print_output(&res, json);
                    Ok(())
                }
                Err(_) => {
                    let deleted = crate::backup::delete_backup(&cfg.state_dir, &name)?;
                    if deleted {
                        println!("backup {name} deleted offline");
                        Ok(())
                    } else {
                        Err(Error::other(format!("backup {name} not found")))
                    }
                }
            }
        }
        Command::RestoreFromZip { file, json, config } => {
            let cfg = Config::load(&config)?;
            if !file.exists() {
                return Err(Error::other(format!("file not found: {}", file.display())));
            }
            let client = ApiClient::new(&cfg);
            match client.upload_zip("/restore/upload", &file).await {
                Ok(res) => {
                    print_output(&res, json);
                    Ok(())
                }
                Err(_) => {
                    println!("daemon unreachable, restoring offline ...");
                    let dir =
                        crate::backup::restore_from_zip(std::fs::File::open(&file)?, &cfg, None)?;
                    let _ = crate::backup::pack_backup_zip(&dir);
                    println!("restore complete. restart daemon to load restored data.");
                    Ok(())
                }
            }
        }
    }
}

fn print_output(val: &serde_json::Value, as_json: bool) {
    if as_json {
        println!("{}", serde_json::to_string_pretty(val).unwrap_or_default());
    } else if let Some(s) = val.as_str() {
        println!("{s}");
    } else {
        println!("{}", serde_json::to_string_pretty(val).unwrap_or_default());
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
