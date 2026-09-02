use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

use serde::Deserialize;
use tokio::sync::RwLock;

use crate::AppError;

const RATES_URL: &str = "https://open.er-api.com/v6/latest/USD";
const TTL_SECS: u64 = 1800; // 30 минут
const BASE: &str = "USD";

#[derive(Deserialize)]
struct RatesResponse {
    result: String,
    #[serde(default)]
    rates: HashMap<String, f64>,
}

/// Курсы к USD с кэшем по TTL. Лениво загружается из внешнего API.
struct Cache {
    rates: HashMap<String, f64>,
    fetched_at: Instant,
}

static CACHE: OnceLock<RwLock<Cache>> = OnceLock::new();

fn cache() -> &'static RwLock<Cache> {
    CACHE.get_or_init(|| {
        RwLock::new(Cache {
            rates: HashMap::new(),
            fetched_at: Instant::now(),
        })
    })
}

async fn fetch_rates() -> Result<HashMap<String, f64>, AppError> {
    let resp = reqwest::get(RATES_URL)
        .await
        .map_err(|e| AppError::internal(format!("не удалось получить курсы валют: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::internal(format!(
            "сервис курсов вернул {}",
            resp.status()
        )));
    }
    let data: RatesResponse = resp
        .json()
        .await
        .map_err(|e| AppError::internal(format!("не удалось разобрать курсы: {e}")))?;
    if data.result != "success" {
        return Err(AppError::internal("сервис курсов вернул ошибку"));
    }
    Ok(data.rates)
}

async fn get_rates() -> Result<HashMap<String, f64>, AppError> {
    {
        let c = cache().read().await;
        if !c.rates.is_empty() && c.fetched_at.elapsed().as_secs() < TTL_SECS {
            return Ok(c.rates.clone());
        }
    }
    let rates = fetch_rates().await?;
    {
        let mut c = cache().write().await;
        c.rates = rates.clone();
        c.fetched_at = Instant::now();
    }
    Ok(rates)
}

fn rate(rates: &HashMap<String, f64>, code: &str) -> Option<f64> {
    let code = code.to_uppercase();
    if code == BASE {
        return Some(1.0);
    }
    rates.get(&code).copied()
}

/// Пересчитывает amount из валюты `from` в валюту `to` через базовую (USD).
pub async fn convert(amount: f64, from: &str, to: &str) -> Result<f64, AppError> {
    let rates = get_rates().await?;
    let from_rate = rate(&rates, from).ok_or_else(|| AppError::bad_request(format!("нет курса для {from}")))?;
    let to_rate = rate(&rates, to).ok_or_else(|| AppError::bad_request(format!("нет курса для {to}")))?;
    // amount (from) -> USD -> to
    let usd = amount / from_rate;
    Ok(usd * to_rate)
}

/// Пересчитывает amount из базовой валюты (USD) в целевую `to`.
pub async fn from_usd(amount: f64, to: &str) -> Result<f64, AppError> {
    convert(amount, BASE, to).await
}

/// Пересчитывает amount из валюты `from` в базовую валюту (USD).
pub async fn to_usd(amount: f64, from: &str) -> Result<f64, AppError> {
    convert(amount, from, BASE).await
}