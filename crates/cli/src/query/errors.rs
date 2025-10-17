use anyhow::Result;
use clap::Args;
use std::collections::{HashMap, HashSet};

use crate::daemon::database::connection::Database;
use crate::daemon::database::types::{Run, RunFilter, RunStatus};
use crate::daemon::speedrun_api::SpeedrunOps;

use super::common::{group_and_display, resolve_game_category};

#[derive(Args)]
pub struct ErrorsArgs {
    #[arg(long, default_value = "20")]
    pub limit: u32,

    #[arg(long)]
    pub error_class: Option<String>,

    #[arg(long)]
    pub group_by: Option<String>,
}

pub async fn handle_errors(db: &Database, ops: &SpeedrunOps, args: ErrorsArgs) -> Result<()> {
    let filter = RunFilter {
        status: Some(RunStatus::Error),
        limit: 1000,
        ..Default::default()
    };

    let mut error_runs = db.query_runs(filter).await?;

    if let Some(error_class) = &args.error_class {
        error_runs.retain(|r| r.error_class.as_deref() == Some(error_class));
    }

    if error_runs.is_empty() {
        println!("No error runs found");
        return Ok(());
    }

    if let Some(group_by) = &args.group_by {
        handle_grouped_errors(ops, &error_runs, group_by, args.limit).await
    } else {
        handle_ungrouped_errors(ops, &error_runs, args.limit).await
    }
}

async fn handle_ungrouped_errors(ops: &SpeedrunOps, error_runs: &[Run], limit: u32) -> Result<()> {
    println!("Recent Errors");
    println!("=============");
    println!();

    for run in error_runs.iter().take(limit as usize) {
        let (game_name, category_name) =
            resolve_game_category(ops, &run.game_id, &run.category_id).await;
        println!("Run ID:       {}", run.run_id);
        println!("Game/Cat:     {} / {}", game_name, category_name);
        println!(
            "Error Class:  {}",
            run.error_class.as_deref().unwrap_or("N/A")
        );
        println!("Retry Count:  {}", run.retry_count);
        if let Some(error_msg) = &run.error_message {
            println!("Error:        {}", error_msg);
        }
        println!();
    }

    Ok(())
}

async fn handle_grouped_errors(
    ops: &SpeedrunOps,
    error_runs: &[Run],
    group_by: &str,
    limit: u32,
) -> Result<()> {
    let unique_pairs: HashSet<(String, String)> = error_runs
        .iter()
        .map(|r| (r.game_id.clone(), r.category_id.clone()))
        .collect();

    let mut names_map: HashMap<(String, String), (String, String)> = HashMap::new();
    for (game_id, category_id) in unique_pairs {
        let (game_name, category_name) = resolve_game_category(ops, &game_id, &category_id).await;
        names_map.insert((game_id, category_id), (game_name, category_name));
    }

    match group_by {
        "message" => group_by_error_message(error_runs, &names_map, limit),
        "class" => group_by_error_class(error_runs, &names_map, limit),
        "category" => group_by_category(error_runs, &names_map, limit),
        _ => Err(anyhow::anyhow!(
            "Invalid group-by value: '{}'. Valid options are: message, class, category",
            group_by
        )),
    }
}

fn group_by_error_message(
    error_runs: &[Run],
    names_map: &HashMap<(String, String), (String, String)>,
    limit: u32,
) -> Result<()> {
    group_and_display(
        error_runs,
        limit,
        "Errors Grouped by Message",
        "unique error messages",
        |run| {
            run.error_message
                .as_deref()
                .unwrap_or("(no message)")
                .to_string()
        },
        |message, runs| {
            println!("Count: {} runs", runs.len());
            println!("Message: {}", message);
            println!("Example runs:");
            for run in runs.iter().take(3) {
                let (game_name, category_name) = names_map
                    .get(&(run.game_id.clone(), run.category_id.clone()))
                    .map(|(g, c)| (g.as_str(), c.as_str()))
                    .unwrap_or((&run.game_id, &run.category_id));
                println!("  - {} ({}/{})", run.run_id, game_name, category_name);
            }
            if runs.len() > 3 {
                println!("  ... and {} more", runs.len() - 3);
            }
            println!();
        },
    )
}

fn group_by_error_class(
    error_runs: &[Run],
    names_map: &HashMap<(String, String), (String, String)>,
    limit: u32,
) -> Result<()> {
    group_and_display(
        error_runs,
        limit,
        "Errors Grouped by Class",
        "unique error classes",
        |run| {
            run.error_class
                .as_deref()
                .unwrap_or("(no class)")
                .to_string()
        },
        |class, runs| {
            println!("Class: {}", class);
            println!("Count: {} runs", runs.len());
            println!("Example runs:");
            for run in runs.iter().take(3) {
                let (game_name, category_name) = names_map
                    .get(&(run.game_id.clone(), run.category_id.clone()))
                    .map(|(g, c)| (g.as_str(), c.as_str()))
                    .unwrap_or((&run.game_id, &run.category_id));
                println!("  - {} ({}/{})", run.run_id, game_name, category_name);
                if let Some(msg) = &run.error_message {
                    let preview = msg.chars().take(60).collect::<String>();
                    println!(
                        "    {}",
                        if msg.len() > 60 {
                            format!("{}...", preview)
                        } else {
                            preview
                        }
                    );
                }
            }
            if runs.len() > 3 {
                println!("  ... and {} more", runs.len() - 3);
            }
            println!();
        },
    )
}

fn group_by_category(
    error_runs: &[Run],
    names_map: &HashMap<(String, String), (String, String)>,
    limit: u32,
) -> Result<()> {
    group_and_display(
        error_runs,
        limit,
        "Errors Grouped by Game/Category",
        "unique game/categories",
        |run| format!("{}/{}", run.game_id, run.category_id),
        |_category, runs| {
            if let Some(first_run) = runs.first() {
                let (game_name, category_name) = names_map
                    .get(&(first_run.game_id.clone(), first_run.category_id.clone()))
                    .map(|(g, c)| (g.as_str(), c.as_str()))
                    .unwrap_or((&first_run.game_id, &first_run.category_id));

                println!("Game/Category: {} / {}", game_name, category_name);
                println!("Count: {} runs", runs.len());

                let mut error_class_counts: HashMap<String, usize> = HashMap::new();
                for run in runs.iter() {
                    let class = run.error_class.as_deref().unwrap_or("(no class)");
                    *error_class_counts.entry(class.to_string()).or_insert(0) += 1;
                }

                println!("Error classes:");
                let mut class_list: Vec<_> = error_class_counts.iter().collect();
                class_list.sort_by(|a, b| b.1.cmp(a.1));
                for (class, count) in class_list {
                    println!("  - {}: {} runs", class, count);
                }

                println!("Example runs:");
                for run in runs.iter().take(3) {
                    println!(
                        "  - {} ({})",
                        run.run_id,
                        run.error_class.as_deref().unwrap_or("N/A")
                    );
                }
                if runs.len() > 3 {
                    println!("  ... and {} more", runs.len() - 3);
                }
                println!();
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_run(
        run_id: &str,
        game_id: &str,
        category_id: &str,
        error_class: Option<&str>,
        error_message: Option<&str>,
    ) -> Run {
        let now = Utc::now();
        Run {
            run_id: run_id.to_string(),
            game_id: game_id.to_string(),
            category_id: category_id.to_string(),
            submitted_date: now,
            status: RunStatus::Error,
            error_message: error_message.map(String::from),
            retry_count: 0,
            next_retry_at: None,
            error_class: error_class.map(String::from),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn test_group_by_error_message() {
        let runs = vec![
            create_test_run("run1", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run2", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run3", "game1", "cat1", Some("final"), Some("error B")),
        ];

        let mut names_map = HashMap::new();
        names_map.insert(
            ("game1".to_string(), "cat1".to_string()),
            ("Game 1".to_string(), "Category 1".to_string()),
        );

        let result = group_by_error_message(&runs, &names_map, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_group_by_error_class() {
        let runs = vec![
            create_test_run("run1", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run2", "game1", "cat1", Some("retryable"), Some("error B")),
            create_test_run("run3", "game1", "cat1", Some("final"), Some("error C")),
        ];

        let mut names_map = HashMap::new();
        names_map.insert(
            ("game1".to_string(), "cat1".to_string()),
            ("Game 1".to_string(), "Category 1".to_string()),
        );

        let result = group_by_error_class(&runs, &names_map, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_group_by_category() {
        let runs = vec![
            create_test_run("run1", "game1", "cat1", Some("final"), Some("error A")),
            create_test_run("run2", "game1", "cat1", Some("final"), Some("error B")),
            create_test_run("run3", "game2", "cat2", Some("retryable"), Some("error C")),
        ];

        let mut names_map = HashMap::new();
        names_map.insert(
            ("game1".to_string(), "cat1".to_string()),
            ("Game 1".to_string(), "Category 1".to_string()),
        );
        names_map.insert(
            ("game2".to_string(), "cat2".to_string()),
            ("Game 2".to_string(), "Category 2".to_string()),
        );

        let result = group_by_category(&runs, &names_map, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_group_by_handles_none_values() {
        let runs = vec![
            create_test_run("run1", "game1", "cat1", None, None),
            create_test_run("run2", "game1", "cat1", Some("final"), Some("error")),
        ];

        let names_map = HashMap::new();

        assert!(group_by_error_message(&runs, &names_map, 10).is_ok());
        assert!(group_by_error_class(&runs, &names_map, 10).is_ok());
        assert!(group_by_category(&runs, &names_map, 10).is_ok());
    }
}
