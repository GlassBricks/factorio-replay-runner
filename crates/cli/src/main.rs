use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use log::{error, info};
use replay_runner::{
    factorio_install_dir::FactorioInstallDir,
    replay_runner::ReplayLog,
    rules::{RunRules, SrcRunRules},
    save_file::SaveFile,
};
use replay_script::MsgType;
use run_downloader::{
    FileDownloader,
    services::{dropbox::DropboxService, gdrive::GoogleDriveService, speedrun::SpeedrunService},
};
use run_replay::run_replay;
use src_integration::{RemoteReplayResult, run_replay_from_src_run};
use std::{
    fmt::Display,
    fs::File,
    path::{Path, PathBuf},
};

mod run_replay;
mod src_integration;

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

    /// GAME rules file (json/yaml)
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

#[tokio::main]
async fn main() -> Result<()> {
    init_logger();
    let args = CliArgs::parse();

    let exit_code = match args.command {
        Commands::Run(sub_args) => cli_run_file(sub_args).await,
        Commands::RunSrc(sub_args) => cli_run_src(sub_args).await,
    }?;

    std::process::exit(exit_code);
}

fn init_logger() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
}

/// Exit codes:
/// 0: Success
/// 1: Warning
/// 2: Error
/// 10: replay run error
async fn cli_run_file(args: RunReplayOnFileArgs) -> Result<i32> {
    let RunReplayOnFileArgs {
        save_file,
        run_rules_file: rules_file,
        install_dir,
        output,
    } = args;
    let output_path = output.unwrap_or_else(|| save_file.with_extension("log"));

    let result = run_file(&save_file, &rules_file, &install_dir, &output_path).await;
    print_result_summary(&result);
    Ok(result_to_exit_code(&result))
}

async fn run_file(
    save_file: &Path,
    rules_file: &Path,
    install_dir: &Path,
    output_path: &Path,
) -> Result<ReplayLog> {
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
    print_result_summary(&result);
    Ok(result_to_exit_code(&result))
}

async fn run_src(
    run_id: &str,
    game_rules_file: &Path,
    install_dir: &Path,
    output_dir: &Path,
) -> RemoteReplayResult {
    let install_dir = load_install_dir(install_dir).await?;
    let rules = load_src_rules(game_rules_file).await?;
    let mut downloader = create_file_downloader().await?;
    run_replay_from_src_run(&mut downloader, run_id, &install_dir, &rules, output_dir).await
}

async fn create_file_downloader() -> Result<FileDownloader> {
    dotenvy::dotenv()?;
    if !std::env::var("AUTO_DOWNLOAD_RUNS").is_ok() {
        panic!(
            "Not downloading runs for security reasons. set AUTO_DOWNLOAD_RUNS=1 to acknowledge risks and enable automatic download"
        );
    }

    Ok(FileDownloader::builder()
        .add_service(GoogleDriveService::from_env().await?)
        .add_service(DropboxService::from_env().await?)
        .add_service(SpeedrunService::new())
        .build())
}

async fn load_install_dir(install_dir_path: &Path) -> Result<FactorioInstallDir> {
    FactorioInstallDir::new_or_create(install_dir_path).with_context(|| {
        format!(
            "Failed to create install directory: {}",
            install_dir_path.display()
        )
    })
}

async fn load_save_file(save_file_path: &Path) -> Result<SaveFile<File>> {
    SaveFile::new(File::open(save_file_path)?)
}

async fn load_run_rules(rules_file_path: &Path) -> Result<RunRules> {
    serde_yaml::from_reader(File::open(rules_file_path)?).with_context(|| "failed to load rules")
}

async fn load_src_rules(game_rules_file_path: &Path) -> Result<SrcRunRules> {
    serde_yaml::from_reader(File::open(game_rules_file_path)?)
        .with_context(|| "failed to load src rules")
}

fn print_result_summary(result: &Result<ReplayLog, impl Display>) {
    match result {
        Ok(replay_log) => {
            if replay_log.exit_success {
                info!("Replay completed successfully!");
            } else {
                error!("Replay failed!");
            }
        }
        Err(err) => {
            error!("{err:#}");
        }
    }
}

fn result_to_exit_code<T>(result: &Result<ReplayLog, T>) -> i32 {
    match result {
        Err(_) => 1,
        Ok(replay_log) => {
            if !replay_log.exit_success {
                10
            } else if replay_log
                .messages
                .iter()
                .any(|msg| msg.msg_type == MsgType::Error)
            {
                1
            } else if replay_log
                .messages
                .iter()
                .any(|msg| msg.msg_type == MsgType::Warn)
            {
                2
            } else {
                0
            }
        }
    }
}

#[cfg(test)]
mod tests;
