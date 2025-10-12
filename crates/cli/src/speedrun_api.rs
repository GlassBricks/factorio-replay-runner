use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

const API_BASE: &str = "https://www.speedrun.com/api/v1";

#[derive(Clone)]
pub struct SpeedrunClient {
    client: Client,
}

impl SpeedrunClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent("factorio-replay-runner")
            .build()
            .context("Failed to create HTTP client")?;
        Ok(Self { client })
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Run> {
        let url = format!("{}/runs/{}", API_BASE, run_id);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            anyhow::bail!("API request failed: {}", response.status());
        }

        let wrapper: RunResponse = response
            .json()
            .await
            .context("Failed to parse run response")?;

        Ok(wrapper.data)
    }

    pub async fn list_runs(&self, query: &RunsQuery) -> Result<Vec<Run>> {
        let mut url = format!("{}/runs", API_BASE);
        let mut params = vec![];

        if let Some(game) = &query.game {
            params.push(format!("game={}", game));
        }
        if let Some(category) = &query.category {
            params.push(format!("category={}", category));
        }
        if let Some(orderby) = &query.orderby {
            params.push(format!("orderby={}", orderby));
        }
        if let Some(direction) = &query.direction {
            params.push(format!("direction={}", direction));
        }
        if let Some(offset) = query.offset {
            params.push(format!("offset={}", offset));
        }
        if let Some(max) = query.max {
            params.push(format!("max={}", max));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            anyhow::bail!("API request failed: {}", response.status());
        }

        let wrapper: RunsResponse = response
            .json()
            .await
            .context("Failed to parse runs response")?;

        Ok(wrapper.data)
    }

    pub async fn stream_runs(&self, query: &RunsQuery) -> Result<Vec<Run>> {
        let mut all_runs = Vec::new();
        let mut offset = 0;
        let page_size = 200;

        loop {
            let mut page_query = query.clone();
            page_query.offset = Some(offset);
            page_query.max = Some(page_size);

            let runs = self.list_runs(&page_query).await?;
            let count = runs.len();
            all_runs.extend(runs);

            if count < page_size {
                break;
            }

            offset += page_size;
        }

        Ok(all_runs)
    }

    pub async fn get_game(&self, game_id: &str) -> Result<Game> {
        let url = format!("{}/games/{}", API_BASE, game_id);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            anyhow::bail!("API request failed: {}", response.status());
        }

        let wrapper: GameResponse = response
            .json()
            .await
            .context("Failed to parse game response")?;

        Ok(wrapper.data)
    }

    pub async fn get_category(&self, category_id: &str) -> Result<Category> {
        let url = format!("{}/categories/{}", API_BASE, category_id);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            anyhow::bail!("API request failed: {}", response.status());
        }

        let wrapper: CategoryResponse = response
            .json()
            .await
            .context("Failed to parse category response")?;

        Ok(wrapper.data)
    }
}

#[derive(Debug, Clone)]
pub struct RunsQuery {
    pub game: Option<String>,
    pub category: Option<String>,
    pub orderby: Option<String>,
    pub direction: Option<String>,
    pub offset: Option<usize>,
    pub max: Option<usize>,
}

impl RunsQuery {
    pub fn new() -> Self {
        Self {
            game: None,
            category: None,
            orderby: None,
            direction: None,
            offset: None,
            max: None,
        }
    }

    pub fn game(mut self, game: impl Into<String>) -> Self {
        self.game = Some(game.into());
        self
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn orderby(mut self, orderby: impl Into<String>) -> Self {
        self.orderby = Some(orderby.into());
        self
    }

    pub fn direction(mut self, direction: impl Into<String>) -> Self {
        self.direction = Some(direction.into());
        self
    }
}

impl Default for RunsQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct RunResponse {
    data: Run,
}

#[derive(Debug, Deserialize)]
struct RunsResponse {
    data: Vec<Run>,
}

#[derive(Debug, Deserialize)]
struct GameResponse {
    data: Game,
}

#[derive(Debug, Deserialize)]
struct CategoryResponse {
    data: Category,
}

#[derive(Debug, Deserialize)]
pub struct Run {
    pub id: String,
    pub game: String,
    pub category: String,
    pub comment: Option<String>,
    pub submitted: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Game {
    pub names: GameNames,
}

#[derive(Debug, Deserialize)]
pub struct GameNames {
    pub international: String,
}

#[derive(Debug, Deserialize)]
pub struct Category {
    pub name: String,
}

pub fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}

#[derive(Clone)]
pub struct SpeedrunOps {
    games: Arc<RwLock<HashMap<String, String>>>,
    categories: Arc<RwLock<HashMap<String, String>>>,
    pub client: SpeedrunClient,
}

impl SpeedrunOps {
    pub fn new(client: &SpeedrunClient) -> Self {
        Self {
            games: Arc::new(RwLock::new(HashMap::new())),
            categories: Arc::new(RwLock::new(HashMap::new())),
            client: client.clone(),
        }
    }

    pub async fn get_game_name(&self, game_id: &str) -> Result<String> {
        {
            let games = self.games.read().await;
            if let Some(name) = games.get(game_id) {
                return Ok(name.clone());
            }
        }

        let game = self.client.get_game(game_id).await?;
        let name = game.names.international;

        self.games.write().await.insert(game_id.to_string(), name.clone());

        Ok(name)
    }

    pub async fn get_category_name(&self, category_id: &str) -> Result<String> {
        {
            let categories = self.categories.read().await;
            if let Some(name) = categories.get(category_id) {
                return Ok(name.clone());
            }
        }

        let category = self.client.get_category(category_id).await?;
        let name = category.name;

        self.categories.write().await.insert(category_id.to_string(), name.clone());

        Ok(name)
    }

    pub async fn format_game_category(&self, game_id: &str, category_id: &str) -> String {
        let game_name = self.get_game_name(game_id).await.unwrap_or_else(|_| game_id.to_string());
        let category_name = self.get_category_name(category_id).await.unwrap_or_else(|_| category_id.to_string());

        format!("{} / {}", game_name, category_name)
    }
}
