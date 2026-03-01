use anyhow::{Context, Result};
use log::LevelFilter;
use replay_script::ReplayScripts;
use std::fs;
use test_utils::{self, workspace_root};

use super::*;

const TEST_RUN_ID: &str = "zngelo7m"; // a steelaxe run
const ALL_RULES_FILE: &str = "all_checks.yaml";

fn init_test_logger() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(LevelFilter::Debug)
        .try_init();
}

fn write_all_checks() {
    let fixtures_dir = test_utils::fixtures_dir();
    let mut all_scripts = ReplayScripts::all_enabled();
    all_scripts.required_research = vec!["steel-axe".to_string()];
    let test_all_rules = RunRules {
        expected_mods_override: Some(
            ["base", "quality", "elevated-rails", "space-age"]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        ),
        replay_scripts: all_scripts,
    };

    let rules_yaml = serde_yaml::to_string(&test_all_rules).unwrap();
    fs::write(fixtures_dir.join("all_checks.yaml"), rules_yaml).unwrap();
}

#[tokio::test]
#[ignore]
async fn test_run_file() -> Result<()> {
    init_test_logger();
    write_all_checks();

    let test_dir = test_utils::test_tmp_dir().join("cli_test");
    let fixtures_dir = test_utils::fixtures_dir();
    let install_dir_path = test_utils::test_factorio_installs_dir();

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir).ok();
    }
    fs::create_dir_all(&test_dir)?;

    let test_save_path = fixtures_dir.join("TEST.zip");
    let rules_file_path = fixtures_dir.join(ALL_RULES_FILE);
    let output_path = test_dir.join("TEST.txt");

    run_file(
        &test_save_path,
        &rules_file_path,
        &install_dir_path,
        &output_path,
    )
    .await?;

    assert!(output_path.exists(), "Output file should be created");

    let output_content = fs::read_to_string(&output_path)?;

    let expected_log_path = fixtures_dir.join("TEST_expected.txt");
    let expected_content = fs::read_to_string(&expected_log_path).with_context(|| {
        format!(
            "Failed to read expected log file: {}",
            expected_log_path.display()
        )
    })?;

    if output_content.trim() != expected_content.trim() {
        let actual_log_path = fixtures_dir.join("TEST_actual.txt");
        fs::write(&actual_log_path, &output_content).ok();
        assert_eq!(
            output_content.trim(),
            expected_content.trim(),
            "Log output should match expected content. Actual output written to TEST_actual.txt"
        );
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_cli_run_src() -> Result<()> {
    init_test_logger();

    let test_dir = test_utils::test_tmp_dir().join("cli_src_test");
    let install_dir_path = test_utils::test_factorio_installs_dir();
    let rules_path = workspace_root().join("speedrun-rules.yaml");

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir).ok();
    }
    fs::create_dir_all(&test_dir)?;

    let database_path = test_dir.join("test.db");
    let result = run_src(
        TEST_RUN_ID,
        &rules_path,
        &install_dir_path,
        &test_dir,
        &database_path,
    )
    .await;

    match result {
        Ok(_) => {
            let output_path = test_dir.join(TEST_RUN_ID).join("output.log");
            assert!(
                output_path.exists(),
                "Output log file should be created on successful run"
            );
        }
        Err(err) => {
            eprintln!("Could not run replay: {}", err);
        }
    }

    Ok(())
}
