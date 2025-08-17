use anyhow::{Context, Result};
use clap::Parser;
use replay_runner::{
    factorio_install_dir::FactorioInstallDir,
    replay_runner::{ReplayLog, RunResult, run_replay_with_rules},
    rules::Rules,
    save_file::SaveFile,
};
use std::io::Write;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(name = "factorio-replay-cli")]
#[command(about = "Run Factorio replays with custom scripts and analyze the results")]
struct Args {
    /// Factorio save file
    save_file: PathBuf,

    /// Rules file (json/yaml)
    rules_file: PathBuf,

    /// Factorio installations directory (defaults to ./factorio_installs)
    /// Installs will created at install_dir/{version}/
    #[arg(long, default_value = "./factorio_installs")]
    install_dir: PathBuf,

    /// Output file path; defaults to save file name with .txt extension
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if !args.save_file.exists() {
        anyhow::bail!("Save file does not exist: {}", args.save_file.display());
    }
    if !args.rules_file.exists() {
        anyhow::bail!("Rules file does not exist: {}", args.rules_file.display());
    }

    println!("Running replay: {}", args.save_file.display());
    println!("Using rules from: {}", args.rules_file.display());
    println!("Factorio install dir: {}", args.install_dir.display());

    let output_path = args
        .output
        .unwrap_or_else(|| args.save_file.with_extension("txt"));

    let result = run_replay_on_file(
        &args.save_file,
        &args.rules_file,
        &args.install_dir,
        &output_path,
    )
    .await?;

    let success = match result {
        RunResult::PreRunCheckFailed(err) => {
            println!("Pre-run check failed: {}", err);
            false
        }
        RunResult::ReplayRan(replay_log) => {
            if replay_log.exit_success {
                println!("Replay completed successfully!");
            } else {
                println!("Replay failed!");
            }
            summarize_results(&replay_log, &output_path);
            replay_log.exit_success
        }
    };

    if !success {
        std::process::exit(1);
    }

    Ok(())
}

fn summarize_results(replay_log: &ReplayLog, replay_log_path: &Path) {
    println!("Results written to: {}", replay_log_path.display());
    println!("{} log messages", replay_log.messages.len());

    let mut info_count = 0;
    let mut warn_count = 0;
    let mut error_count = 0;

    for msg in &replay_log.messages {
        match msg.msg_type {
            replay_script::MsgType::Info => info_count += 1,
            replay_script::MsgType::Warn => warn_count += 1,
            replay_script::MsgType::Error => error_count += 1,
        }
    }

    let result = if error_count > 0 {
        "err"
    } else if warn_count > 0 {
        "warn"
    } else {
        "ok"
    };

    println!("Overall result: {result}");
    println!("Summary: {error_count} errors, {warn_count} warnings, {info_count} info messages");
}

async fn run_replay_on_file(
    save_file_path: &Path,
    rules_file_path: &Path,
    install_dir_path: &Path,
    output_path: &Path,
) -> Result<RunResult> {
    let save_file_handle = File::open(save_file_path)
        .with_context(|| format!("Failed to open save file: {}", save_file_path.display()))?;

    let mut save_file = SaveFile::new(save_file_handle)
        .with_context(|| format!("Failed to load save file: {}", save_file_path.display()))?;

    let install_dir = FactorioInstallDir::new_or_create(install_dir_path).with_context(|| {
        format!(
            "Failed to create install directory: {}",
            install_dir_path.display()
        )
    })?;

    let reader = File::open(rules_file_path)
        .with_context(|| format!("Failed to open rules file: {}", rules_file_path.display()))?;

    let rules: Rules = serde_yaml::from_reader(reader).with_context(|| {
        format!(
            "Failed to parse rules file as YAML: {}",
            rules_file_path.display()
        )
    })?;

    let result = run_replay_with_rules(&install_dir, &mut save_file, &rules)
        .await
        .context("Failed to run replay with rules")?;

    if let RunResult::ReplayRan(ref replay_log) = result {
        write_replay_log(replay_log, output_path).context("Failed to write replay log")?;
    }

    Ok(result)
}

fn write_replay_log(
    replay_log: &replay_runner::replay_runner::ReplayLog,
    output_path: &Path,
) -> Result<()> {
    let mut file = File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;

    for msg in &replay_log.messages {
        writeln!(file, "[{}] {} {}", msg.msg_type, msg.time, msg.message)
            .context("Failed to write message to output file")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use replay_runner::rules::ExpectedMods;
    use replay_script::ReplayScripts;
    use test_utils;

    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_run_replay_with_all_rules() -> Result<()> {
        let test_dir = test_utils::test_tmp_dir().join("cli_test");
        let fixtures_dir = test_utils::fixtures_dir();
        let install_dir_path = test_utils::test_factorio_installs_dir();

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }
        fs::create_dir_all(&test_dir)?;

        let test_save_path = fixtures_dir.join("TEST.zip");
        let rules_file_path = fixtures_dir.join("all_rules.yaml");
        let output_path = test_dir.join("TEST.txt");

        let result = run_replay_on_file(
            &test_save_path,
            &rules_file_path,
            &install_dir_path,
            &output_path,
        )
        .await?;

        let replay_log = match result {
            RunResult::ReplayRan(log) => log,
            RunResult::PreRunCheckFailed(err) => {
                panic!("Pre-run check failed: {}", err);
            }
        };

        assert!(replay_log.exit_success, "Replay should exit successfully");

        assert!(output_path.exists(), "Output file should be created");

        let output_content = fs::read_to_string(&output_path)?;

        let expected_log_path = fixtures_dir.join("TEST_expected.txt");
        let expected_content = fs::read_to_string(&expected_log_path).with_context(|| {
            format!(
                "Failed to read expected log file: {}",
                expected_log_path.display()
            )
        })?;

        assert_eq!(
            output_content.trim(),
            expected_content.trim(),
            "Log output should match expected content"
        );

        Ok(())
    }

    #[test]
    #[ignore]
    fn write_all_rules_to_fixtures() {
        let fixtures_dir = test_utils::fixtures_dir();
        let all_scripts = ReplayScripts::all_enabled();
        let test_all_rules = Rules {
            expected_mods: ExpectedMods::SpaceAge,
            checks: all_scripts,
        };

        let rules_yaml_path = fixtures_dir.join("all_rules.yaml");
        let rules_yaml = serde_yaml::to_string(&test_all_rules).unwrap();
        fs::write(rules_yaml_path, rules_yaml).unwrap();
    }
}
