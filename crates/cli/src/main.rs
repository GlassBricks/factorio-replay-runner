use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use config::{DaemonConfig, RunRules, SrcRunRules};
use factorio_manager::{
    factorio_install_dir::FactorioInstallDir,
    process_manager::GLOBAL_PROCESS_MANAGER,
    save_file::{SaveFile, WrittenSaveFile},
    shutdown::ShutdownCoordinator,
};
use log::info;
use run_replay::{ReplayReport, run_replay};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::run_processing::{RunProcessingContext, download_and_run_replay};

mod config;
mod daemon;
mod database;
mod error;
mod run_processing;
mod run_replay;
mod speedrun_api;

#[derive(Parser)]
#[command(name = "factorio-replay-cli")]
#[command(about = "Run Factorio replays with custom scripts and analyze the results")]
struct CliArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run(RunReplayOnFileArgs),
    RunSrc(RunReplayFromSrcArgs),
    Daemon(DaemonArgs),
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
    init_logger();

    let args = CliArgs::parse();

    let coordinator = ShutdownCoordinator::new(Arc::new((*GLOBAL_PROCESS_MANAGER).clone()));
    coordinator.setup_handlers()?;

    match args.command {
        Commands::Run(sub_args) => {
            let exit_code = cli_run_file(sub_args).await?;
            std::process::exit(exit_code);
        }
        Commands::RunSrc(sub_args) => {
            let exit_code = cli_run_src(sub_args).await?;
            std::process::exit(exit_code);
        }
        Commands::Daemon(sub_args) => {
            cli_daemon(sub_args, coordinator).await?;
            Ok(())
        }
    }
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
    let db = database::connection::Database::new(database).await?;
    let client = speedrun_api::SpeedrunClient::new()?;
    let speedrun_ops = speedrun_api::SpeedrunOps::new(&client);

    info!("Fetching run data (https://speedrun.com/runs/{})", run_id);
    let (fetched_run_id, game_id, category_id, submitted_date) =
        run_processing::fetch_run_details(&client, run_id).await?;

    let game_category = speedrun_ops
        .format_game_category(&game_id, &category_id)
        .await;
    info!("Game: {}", game_category);

    let (run_rules, expected_mods) = src_rules.resolve_rules(&game_id, &category_id)?;

    let new_run = database::types::NewRun::new(
        fetched_run_id.clone(),
        game_id.clone(),
        category_id.clone(),
        submitted_date,
    );
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

    db.mark_run_processing(&fetched_run_id).await?;

    let result = download_and_run_replay(
        &client,
        &fetched_run_id,
        run_rules,
        expected_mods,
        install_dir,
        output_dir,
    )
    .await;

    let report = result.as_ref().ok().cloned();
    db.process_replay_result(&fetched_run_id, result).await?;

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
    let db = database::connection::Database::new(database).await?;
    let client = speedrun_api::SpeedrunClient::new()?;
    let speedrun_ops = speedrun_api::SpeedrunOps::new(&client);

    std::fs::create_dir_all(install_dir)?;
    std::fs::create_dir_all(output_dir)?;

    let ctx = RunProcessingContext {
        db,
        speedrun_ops,
        src_rules,
        install_dir: install_dir.to_path_buf(),
        output_dir: output_dir.to_path_buf(),
    };

    info!("Polling speedrun.com for new runs");
    let work_notify = Arc::new(tokio::sync::Notify::new());
    daemon::poll_speedrun_com(&ctx, &daemon_config, &work_notify).await?;

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

async fn cli_daemon(args: DaemonArgs, coordinator: ShutdownCoordinator) -> Result<i32> {
    let DaemonArgs { config } = args;

    let daemon_config = load_daemon_config(&config).await?;
    let src_rules = load_src_rules(&daemon_config.game_rules_file).await?;

    daemon::run_daemon(daemon_config, src_rules, coordinator).await?;
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

async fn load_daemon_config(path: &Path) -> Result<DaemonConfig> {
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
