use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use config::RunRules;
use factorio_manager::{
    factorio_install_dir::FactorioInstallDir,
    save_file::{SaveFile, WrittenSaveFile},
};
use log::info;
use run_replay::{ReplayReport, run_replay};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::signal;
use tokio::signal::unix::SignalKind;
use tokio_util::sync::CancellationToken;

use crate::daemon::{RunProcessingContext, SrcRunRules, download_and_run_replay};

mod admin;
mod config;
mod daemon;
mod error;
mod query;
mod run_replay;

#[derive(Parser)]
#[command(name = "factorio-replay-cli")]
#[command(about = "Run Factorio replays with custom scripts and analyze the results")]
struct CliArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a replay from a local save file
    Run(RunReplayOnFileArgs),
    /// Run a replay fetched from speedrun.com
    RunSrc(RunReplayFromSrcArgs),
    /// Start the daemon to poll and process speedrun.com runs
    Daemon(DaemonArgs),
    /// Query the database for run information
    Query(query::QueryArgs),
    /// Administrative database operations
    Admin(admin::AdminArgs),
}

#[derive(Args)]
struct RunReplayOnFileArgs {
    /// Factorio save file
    save: PathBuf,

    /// RUN Rules (json/yaml)
    run_rules: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at {install_dir}/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output file; defaults to save file name with .txt extension
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct RunReplayFromSrcArgs {
    /// Run id (if not provided, polls speedrun.com once and processes one run)
    run_id: Option<String>,

    /// GAME rules (yaml)
    #[arg(default_value = "./speedrun_rules.yaml")]
    game_rules: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at {install_dir}/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output directory; defaults to ./src_runs
    /// Files will be written to {output_dir}/{run_id}/
    ///     {save_name}.zip
    ///     {log}.txt
    #[arg(short, long, default_value = "./src_runs")]
    output_dir: PathBuf,

    /// SQLite database for tracking run status
    #[arg(long, default_value = "run_verification.db")]
    database: PathBuf,
}

#[derive(Args)]
struct DaemonArgs {
    /// Daemon configuration (yaml)
    #[arg(short, long, default_value = "./daemon.yaml")]
    config: PathBuf,
}
#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_logger();

    let token = setup_signal_handler()?;
    let args = CliArgs::parse();

    match args.command {
        Commands::Run(sub_args) => {
            let exit_code = tokio::select! {
                result = cli_run_file(sub_args) => result?,
                _ = token.cancelled() => { log::info!("Interrupted"); 130 }
            };
            std::process::exit(exit_code);
        }
        Commands::RunSrc(sub_args) => {
            let exit_code = tokio::select! {
                result = cli_run_src(sub_args) => result?,
                _ = token.cancelled() => { log::info!("Interrupted"); 130 }
            };
            std::process::exit(exit_code);
        }
        Commands::Daemon(sub_args) => {
            cli_daemon(sub_args, token).await?;
            Ok(())
        }
        Commands::Query(sub_args) => {
            query::handle_query_command(sub_args).await?;
            Ok(())
        }
        Commands::Admin(sub_args) => {
            admin::handle_admin_command(sub_args).await?;
            Ok(())
        }
    }
}

fn setup_signal_handler() -> Result<CancellationToken> {
    let token = CancellationToken::new();
    let cloned = token.clone();
    tokio::spawn(async move {
        let mut sigint = signal::unix::signal(SignalKind::interrupt())?;
        let mut sigterm = signal::unix::signal(SignalKind::terminate())?;
        tokio::select! {
            _ = sigint.recv() => log::info!("Received SIGINT, shutting down..."),
            _ = sigterm.recv() => log::info!("Received SIGTERM, shutting down..."),
        }
        cloned.cancel();
        Ok::<(), std::io::Error>(())
    });
    Ok(token)
}

fn init_logger() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
}

async fn cli_run_file(args: RunReplayOnFileArgs) -> Result<i32> {
    let RunReplayOnFileArgs {
        save,
        run_rules,
        install_dir,
        output,
    } = args;
    let output_path = output.unwrap_or_else(|| save.with_extension("log"));

    let result = run_file(&save, &run_rules, &install_dir, &output_path).await;
    Ok(result_to_exit_code(&result))
}

async fn run_file(
    save: &Path,
    rules: &Path,
    install_dir: &Path,
    output: &Path,
) -> Result<ReplayReport> {
    let install_dir = load_install_dir(install_dir).await?;
    let mut save_file = load_save(save).await?;
    let rules = load_run_rules(rules).await?;
    run_replay(
        &install_dir,
        &mut save_file,
        &rules,
        rules
            .expected_mods_override
            .as_ref()
            .expect("Expected mods is required for basic rules"),
        output,
    )
    .await
    .map_err(anyhow::Error::from)
}

async fn cli_run_src(args: RunReplayFromSrcArgs) -> Result<i32> {
    let RunReplayFromSrcArgs {
        run_id,
        game_rules,
        install_dir,
        output_dir,
        database,
    } = args;

    match run_id {
        Some(run_id) => {
            let result = run_src(&run_id, &game_rules, &install_dir, &output_dir, &database).await;
            Ok(result_to_exit_code(&result))
        }
        None => run_src_once(&game_rules, &install_dir, &output_dir, &database).await,
    }
}

async fn run_src(
    run_id: &str,
    game_rules: &Path,
    install_dir: &Path,
    output_dir: &Path,
    database: &Path,
) -> Result<ReplayReport> {
    let src_rules = load_src_rules(game_rules).await?;
    let db = daemon::database::connection::Database::new(database).await?;
    let client = daemon::speedrun_api::SpeedrunClient::new()?;
    let speedrun_ops = daemon::speedrun_api::SpeedrunOps::new(&client);

    info!("Fetching run data (https://speedrun.com/runs/{})", run_id);
    let run = client.get_run(run_id).await?;
    let submitted_date = run.get_submitted_date()?;

    let game_category = speedrun_ops
        .format_game_category(&run.game, &run.category)
        .await;
    info!("Game: {}", game_category);

    let (run_rules, expected_mods) = src_rules.resolve_rules(&run.game, &run.category)?;
    let run_id = run.id;

    let new_run =
        daemon::database::types::NewRun::new(&run_id, run.game, run.category, submitted_date);
    db.insert_run(new_run)
        .await
        .or_else(|e| {
            if e.to_string().contains("UNIQUE constraint failed") {
                info!("Run already exists in database");
                Ok(())
            } else {
                Err(e)
            }
        })
        .context("Failed to insert run into database")?;

    db.mark_run_processing(&run_id).await?;

    let result = download_and_run_replay(
        &client,
        &run_id,
        run_rules,
        expected_mods,
        install_dir,
        output_dir,
    )
    .await;

    let report = result.as_ref().ok().cloned();
    let retry_config = daemon::retry::RetryConfig::default();
    db.process_replay_result(&run_id, result, &retry_config)
        .await?;

    report.ok_or_else(|| anyhow::anyhow!("Failed to process replay"))
}

async fn run_src_once(
    game_rules: &Path,
    install_dir: &Path,
    output_dir: &Path,
    database: &Path,
) -> Result<i32> {
    let daemon_config = load_daemon_config(&PathBuf::from("./daemon.yaml"))
        .await
        .context("Failed to load daemon config")?;
    let src_rules = load_src_rules(game_rules).await?;
    let db = daemon::database::connection::Database::new(database).await?;
    let client = daemon::speedrun_api::SpeedrunClient::new()?;
    let speedrun_ops = daemon::speedrun_api::SpeedrunOps::new(&client);

    std::fs::create_dir_all(install_dir)?;
    std::fs::create_dir_all(output_dir)?;

    let ctx = RunProcessingContext {
        db,
        speedrun_ops,
        src_rules,
        install_dir: install_dir.to_path_buf(),
        output_dir: output_dir.to_path_buf(),
        retry_config: daemon_config.retry.clone(),
        bot_notifier: None,
    };

    info!("Polling speedrun.com for new runs");
    let work_notify = Arc::new(tokio::sync::Notify::new());
    daemon::poll_speedrun_com(&ctx, &daemon_config.polling, &work_notify).await?;

    info!("Processing one run from queue");
    match daemon::find_run_to_process(&ctx).await? {
        daemon::ProcessResult::Processed => {
            info!("Successfully processed one run");
            Ok(0)
        }
        daemon::ProcessResult::NoWork => {
            info!("No runs available to process");
            Ok(0)
        }
    }
}

async fn cli_daemon(args: DaemonArgs, token: CancellationToken) -> Result<i32> {
    let DaemonArgs { config } = args;

    let daemon_config = load_daemon_config(&config).await?;
    let src_rules = load_src_rules(&daemon_config.game_rules_file).await?;

    daemon::run_daemon(daemon_config, src_rules, token).await?;
    Ok(0)
}

async fn load_install_dir(path: &Path) -> Result<FactorioInstallDir> {
    FactorioInstallDir::new_or_create(path)
        .with_context(|| format!("Failed to create install directory: {}", path.display()))
}

async fn load_save(path: &Path) -> Result<WrittenSaveFile> {
    Ok(WrittenSaveFile(
        path.to_path_buf(),
        SaveFile::new(File::open(path)?)?,
    ))
}

async fn load_run_rules(path: &Path) -> Result<RunRules> {
    serde_yaml::from_reader(File::open(path)?).with_context(|| "failed to load rules")
}

async fn load_src_rules(path: &Path) -> Result<SrcRunRules> {
    serde_yaml::from_reader(File::open(path)?).with_context(|| "failed to load src rules")
}

async fn load_daemon_config(path: &Path) -> Result<daemon::DaemonConfig> {
    serde_yaml::from_reader(File::open(path)?).with_context(|| "failed to load daemon config")
}

fn result_to_exit_code<T>(result: &Result<ReplayReport, T>) -> i32 {
    match result {
        Ok(report) => report.to_exit_code(),
        Err(_) => 20,
    }
}

#[cfg(test)]
mod tests;
