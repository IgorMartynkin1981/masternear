use common::AppResult;
use common::AppError;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Coords {
    pub lat: f64,
    pub lng: f64,
}

const NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/search";

pub async fn geocode(place: &str) -> AppResult<Coords> {
    let client = reqwest::Client::builder()
        .user_agent("MasterNear/1.0")
        .build()
        .map_err(|e| AppError::internal(format!("ошибка http-клиента: {e}")))?;

    let url = reqwest::Url::parse_with_params(
        NOMINATIM_URL,
        &[("q", place), ("format", "json"), ("limit", "1")],
    )
    .map_err(|e| AppError::internal(format!("ошибка url: {e}")))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::internal(format!("геокодер недоступен: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::internal(format!(
            "геокодер вернул {}",
            resp.status()
        )));
    }

    let items: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| AppError::internal(format!("не удалось разобрать геокодер: {e}")))?;

    let first = items
        .first()
        .ok_or_else(|| AppError::bad_request("не удалось найти указанное место"))?;

    let lat = first
        .get("lat")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| AppError::internal("геокодер не вернул широту"))?;
    let lng = first
        .get("lon")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or_else(|| AppError::internal("геокодер не вернул долготу"))?;

    Ok(Coords { lat, lng })
}
