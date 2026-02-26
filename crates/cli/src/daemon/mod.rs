use anyhow::{Context, Result};
use log::info;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub mod bot_notifier;
pub mod config;
pub mod database;
pub mod poller;
pub mod processor;
pub mod retry;
pub mod run_processing;
pub mod speedrun_api;

pub use bot_notifier::BotNotifierHandle;
pub use config::{DaemonConfig, SrcRunRules};
pub use poller::{poll_speedrun_com, poll_speedrun_com_loop};
pub use processor::{ProcessResult, find_run_to_process, process_runs_loop};
pub use run_processing::{RunProcessingContext, download_and_run_replay};
pub use speedrun_api::{SpeedrunClient, SpeedrunOps};

pub async fn run_daemon(
    config: DaemonConfig,
    src_rules: SrcRunRules,
    token: CancellationToken,
) -> Result<()> {
    info!("Starting daemon with config: {:?}", config);
    info!("Monitoring {} game(s)", src_rules.games.len());

    let db = database::connection::Database::new(&config.database_path)
        .await
        .context("Failed to initialize database")?;

    let client = SpeedrunClient::new()?;
    let speedrun_ops = SpeedrunOps::new(&client).with_db(db.clone());

    std::fs::create_dir_all(&config.install_dir)?;
    std::fs::create_dir_all(&config.output_dir)?;

    let work_notify = Arc::new(Notify::new());

    let bot_notifier = if let Some(cfg) = &config.bot_notifier {
        let (handle, rx) = BotNotifierHandle::new();
        let actor_db = db.clone();
        let actor_cfg = cfg.clone();
        let actor_token = token.clone();
        tokio::spawn(bot_notifier::run_bot_notifier_actor(
            rx,
            actor_db,
            actor_cfg,
            actor_token,
        ));
        Some(handle)
    } else {
        None
    };

    info!("Daemon started successfully");

    let ctx = RunProcessingContext {
        db,
        speedrun_ops,
        src_rules,
        install_dir: config.install_dir,
        output_dir: config.output_dir,
        retry_config: config.retry,
        bot_notifier,
    };

    let poller = poll_speedrun_com_loop(
        ctx.clone(),
        config.polling,
        work_notify.clone(),
        token.clone(),
    );
    let processor = process_runs_loop(ctx, work_notify.clone(), token);

    let (poller_result, processor_result) = tokio::join!(poller, processor);
    poller_result.and(processor_result)?;

    info!("Daemon shutting down");
    Ok(())
}
