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

use crate::run_processing::{download_and_run_replay, fetch_run_metadata};

mod config;
mod daemon;
mod database;
mod run_processing;
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
    Run(RunReplayOnFileArgs),
    RunSrc(RunReplayFromSrcArgs),
    Daemon(DaemonArgs),
}

#[derive(Args)]
struct RunReplayOnFileArgs {
    /// Factorio save file
    save_file: PathBuf,

    /// RUN Rules file (json/yaml)
    run_rules_file: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at {install_dir}/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output file path; defaults to save file name with .txt extension
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct RunReplayFromSrcArgs {
    /// Run id
    run_id: String,

    /// GAME rules file (yaml)
    #[arg(default_value = "./speedrun_rules.yaml")]
    game_rules_file: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at {install_dir}/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output path; defaults to ./src_runs
    /// Files will be written to {output_dir}/{run_id}/
    ///     {save_name}.zip
    ///     {log}.txt
    #[arg(short, long, default_value = "./src_runs")]
    output_dir: PathBuf,
}

#[derive(Args)]
struct DaemonArgs {
    /// Daemon configuration file (yaml)
    #[arg(short, long, default_value = "./daemon.yaml")]
    config_file: PathBuf,
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
        save_file,
        run_rules_file: rules_file,
        install_dir,
        output,
    } = args;
    let output_path = output.unwrap_or_else(|| save_file.with_extension("log"));

    let result = run_file(&save_file, &rules_file, &install_dir, &output_path).await;
    Ok(result_to_exit_code(&result))
}

async fn run_file(
    save_file: &Path,
    rules_file: &Path,
    install_dir: &Path,
    output_path: &Path,
) -> Result<ReplayReport> {
    let install_dir = load_install_dir(install_dir).await?;
    let mut save_file = load_save_file(save_file).await?;
    let rules = load_run_rules(rules_file).await?;
    run_replay(
        &install_dir,
        &mut save_file,
        &rules,
        rules
            .expected_mods_override
            .as_ref()
            .expect("Expected mods is required for basic rules"),
        output_path,
    )
    .await
}

async fn cli_run_src(args: RunReplayFromSrcArgs) -> Result<i32> {
    let RunReplayFromSrcArgs {
        run_id,
        game_rules_file,
        install_dir,
        output_dir,
    } = args;

    let result = run_src(&run_id, &game_rules_file, &install_dir, &output_dir).await;
    Ok(result_to_exit_code(&result))
}

async fn run_src(
    run_id: &str,
    game_rules_file: &Path,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ReplayReport> {
    let rules = load_src_rules(game_rules_file).await?;

    info!("Fetching run data (https://speedrun.com/runs/{})", run_id);
    let (game_id, category_id) = fetch_run_metadata(run_id).await?;
    let game_config = rules
        .games
        .get(&game_id)
        .ok_or_else(|| anyhow::anyhow!("No rules configured for game ID: {}", game_id))?;
    let category_config = game_config
        .categories
        .get(&category_id)
        .ok_or_else(|| anyhow::anyhow!("No rules configured for category ID: {}", category_id))?;
    let game_name = game_config.name.as_deref().unwrap_or(&game_id);
    let category_name = category_config.name.as_deref().unwrap_or(&category_id);
    info!("Game: {}, Category: {}", game_name, category_name);
    let (run_rules, expected_mods) = rules.resolve_rules(&game_id, &category_id)?;

    download_and_run_replay(run_id, run_rules, expected_mods, install_dir, output_dir).await
}

async fn cli_daemon(args: DaemonArgs, coordinator: ShutdownCoordinator) -> Result<i32> {
    let DaemonArgs { config_file } = args;

    let daemon_config = load_daemon_config(&config_file).await?;
    let src_rules = load_src_rules(&daemon_config.game_rules_file).await?;

    daemon::run_daemon(daemon_config, src_rules.games, coordinator).await?;
    Ok(0)
}

async fn load_install_dir(install_dir_path: &Path) -> Result<FactorioInstallDir> {
    FactorioInstallDir::new_or_create(install_dir_path).with_context(|| {
        format!(
            "Failed to create install directory: {}",
            install_dir_path.display()
        )
    })
}

async fn load_save_file(save_file_path: &Path) -> Result<WrittenSaveFile> {
    Ok(WrittenSaveFile(
        save_file_path.to_path_buf(),
        SaveFile::new(File::open(save_file_path)?)?,
    ))
}

async fn load_run_rules(rules_file_path: &Path) -> Result<RunRules> {
    serde_yaml::from_reader(File::open(rules_file_path)?).with_context(|| "failed to load rules")
}

async fn load_src_rules(game_rules_file_path: &Path) -> Result<SrcRunRules> {
    serde_yaml::from_reader(File::open(game_rules_file_path)?)
        .with_context(|| "failed to load src rules")
}

async fn load_daemon_config(config_file_path: &Path) -> Result<DaemonConfig> {
    serde_yaml::from_reader(File::open(config_file_path)?)
        .with_context(|| "failed to load daemon config")
}

fn result_to_exit_code<T>(result: &Result<ReplayReport, T>) -> i32 {
    match result {
        Ok(report) => report.to_exit_code(),
        Err(_) => 20,
    }
}

#[cfg(test)]
mod tests;
