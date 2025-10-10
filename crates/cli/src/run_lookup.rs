use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use log::{debug, info};
use speedrun_api::api;
use speedrun_api::api::{runs::RunStatus, PagedEndpointExt};
use speedrun_api::SpeedrunApiClientAsync;

use crate::database::types::NewRun;

pub async fn poll_game_category(
    game_id: &str,
    category_id: &str,
    last_poll_time: &str,
    cutoff_date: &str,
) -> Result<Vec<NewRun>> {
    info!(
        "Polling for new runs: game={}, category={}",
        game_id, category_id
    );

    let client = SpeedrunApiClientAsync::new()?;

    let last_poll_dt = parse_datetime(last_poll_time)?;
    let cutoff_dt = parse_datetime(cutoff_date)?;

    let endpoint = api::runs::Runs::builder()
        .game(game_id)
        .category(category_id)
        .status(RunStatus::Verified)
        .orderby(api::runs::RunsSorting::Submitted)
        .direction(api::Direction::Asc)
        .build()?;

    let mut stream = endpoint.stream(&client);
    let mut new_runs = Vec::new();

    while let Some(result) = stream.next().await {
        let run: speedrun_api::types::Run = result?;

        let submitted_dt = run
            .submitted
            .as_ref()
            .and_then(|s| parse_datetime(s).ok());

        let should_include = submitted_dt
            .map(|dt| dt > last_poll_dt && dt >= cutoff_dt)
            .unwrap_or(false);

        if !should_include {
            continue;
        }

        let runner_name = extract_runner_name(&run);
        let submitted_date = run.submitted.clone().unwrap_or_default();

        let new_run = NewRun::new(run.id.to_string(), game_id, category_id, submitted_date)
            .with_runner(runner_name.unwrap_or_else(|| "Unknown".to_string()));

        new_runs.push(new_run);
    }

    debug!("Found {} new runs", new_runs.len());
    Ok(new_runs)
}

fn extract_runner_name(run: &speedrun_api::types::Run<'_>) -> Option<String> {
    let player = run.players.first()?;

    Some(match player {
        speedrun_api::types::Player::User { id, .. } => id.to_string(),
        speedrun_api::types::Player::Guest { name, .. } => name.clone(),
    })
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}
