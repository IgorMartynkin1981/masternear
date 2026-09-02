use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use bcrypt::hash;
use chrono::{DateTime, NaiveDate, Utc};
use common::AppResult;
use common::{AppError, require_auth};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::currencies;
use crate::state::AppState;

/// Допустимые значения для пола (гендерная принадлежность).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Gender {
    #[default]
    Unspecified,
    Male,
    Female,
    Other,
}

/// Язык интерфейса.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Ru,
    En,
}

impl Gender {
    pub fn as_str(&self) -> &'static str {
        match self {
            Gender::Unspecified => "unspecified",
            Gender::Male => "male",
            Gender::Female => "female",
            Gender::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Gender> {
        match s.trim().to_ascii_lowercase().as_str() {
            "male" => Some(Gender::Male),
            "female" => Some(Gender::Female),
            "other" => Some(Gender::Other),
            "unspecified" | "" => Some(Gender::Unspecified),
            _ => None,
        }
    }
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Ru => "ru",
            Language::En => "en",
        }
    }

    pub fn parse(s: &str) -> Option<Language> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ru" | "" => Some(Language::Ru),
            "en" => Some(Language::En),
            _ => None,
        }
    }
}

/// Популярные часовые пояса (IANA), предлагаемые в настройках.
pub const TIMEZONES: &[&str] = &[
    "Europe/Minsk",
    "Europe/Moscow",
    "Europe/Kiev",
    "Europe/Berlin",
    "Europe/Paris",
    "Europe/London",
    "Europe/Madrid",
    "Europe/Rome",
    "Asia/Tbilisi",
    "Asia/Yerevan",
    "Asia/Baku",
    "Asia/Yekaterinburg",
    "Asia/Novosibirsk",
    "Asia/Krasnoyarsk",
    "Asia/Irkutsk",
    "Asia/Vladivostok",
    "Asia/Shanghai",
    "Asia/Dubai",
    "Asia/Tokyo",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "America/Toronto",
    "America/Sao_Paulo",
    "America/Mexico_City",
    "Africa/Cairo",
    "Africa/Lagos",
    "UTC",
];

/// Минимально валидный номер телефона: 9–15 цифр, остальные символы — формат.
pub fn normalize_phone(phone: &str) -> Result<Option<String>, String> {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Ok(None);
    }
    let len = digits.len();
    if !(9..=15).contains(&len) {
        return Err("телефон должен содержать от 9 до 15 цифр".to_string());
    }
    Ok(Some(format!("+{digits}")))
}

/// Профиль аккаунта (персональные данные и предпочтения пользователя).
#[derive(Serialize, FromRow)]
pub struct ProfileDto {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: String,
    pub gender: String,
    pub date_of_birth: Option<NaiveDate>,
    pub phone: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub address: Option<String>,
    pub timezone: Option<String>,
    pub language: String,
    pub currency: String,
    pub avatar_url: Option<String>,
    pub notify_email: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct ProfileResp {
    pub profile: ProfileDto,
    pub timezones: Vec<&'static str>,
    pub languages: Vec<&'static str>,
    pub genders: Vec<&'static str>,
    pub currencies: Vec<currencies::Currency>,
}

#[derive(Default, Deserialize)]
pub struct UpdateProfileReq {
    pub name: Option<String>,
    pub email: Option<String>,
    pub gender: Option<String>,
    pub date_of_birth: Option<String>,
    pub phone: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub address: Option<String>,
    pub timezone: Option<String>,
    pub language: Option<String>,
    pub currency: Option<String>,
    pub avatar_url: Option<String>,
    pub notify_email: Option<bool>,
}

pub async fn profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<ProfileResp>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let profile = load_profile(&state.pool, claims.sub).await?;

    Ok(Json(resp(profile)))
}

pub async fn update_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpdateProfileReq>,
) -> AppResult<Json<ProfileResp>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    let current = load_profile(&state.pool, claims.sub).await?;

    // Парсинг и валидация полей.
    let gender = match req.gender {
        Some(s) => Gender::parse(&s).ok_or_else(|| AppError::bad_request("недопустимое значение пола"))?,
        None => Gender::Unspecified,
    };

    let dob = match req.date_of_birth {
        Some(s) if !s.trim().is_empty() => {
            let d = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                .map_err(|_| AppError::bad_request("дата рождения в формате ГГГГ-ММ-ДД"))?;
            if d > Utc::now().date_naive() {
                return Err(AppError::bad_request("дата рождения не может быть в будущем"));
            }
            let age = (Utc::now().date_naive() - d).num_days();
            if age > 120 * 365 {
                return Err(AppError::bad_request("проверьте дату рождения"));
            }
            Some(d)
        }
        Some(_) => None,
        None => None,
    };

    let phone = match req.phone {
        Some(s) if !s.trim().is_empty() => {
            let p = normalize_phone(&s).map_err(AppError::bad_request)?;
            Some(p.unwrap_or_default())
        }
        _ => None,
    };

    let timezone = match req.timezone {
        Some(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    };
    if let Some(tz) = timezone.as_deref()
        && !TIMEZONES.contains(&tz)
    {
        return Err(AppError::bad_request(format!("неизвестный часовой пояс: {tz}")));
    }

    let language = match req.language {
        Some(s) => Language::parse(&s).ok_or_else(|| AppError::bad_request("недопустимый язык"))?,
        None => Language::Ru,
    };

    let currency = match req.currency {
        Some(s) => {
            let code = s.trim().to_uppercase();
            if currencies::find(&code).is_none() {
                return Err(AppError::bad_request(format!("неизвестная валюта: {code}")));
            }
            Some(code)
        }
        None => None,
    };

    let notify_email = req.notify_email;

    let name = req
        .name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Смена email: валидация формата и проверка уникальности.
    let email = match req.email {
        Some(s) => {
            let e = s.trim().to_lowercase();
            if e.is_empty() {
                None
            } else {
                let valid = e.contains('@')
                    && e.split('@').count() == 2
                    && !e.starts_with('@')
                    && !e.ends_with('@')
                    && e.rsplit('@').next().unwrap_or_default().contains('.');
                if !valid {
                    return Err(AppError::bad_request("введите корректный email"));
                }
                if e != current.email {
                    let taken: Option<i64> =
                        sqlx::query_scalar("SELECT id FROM users WHERE email = $1 AND id <> $2")
                            .bind(&e)
                            .bind(claims.sub)
                            .fetch_optional(&state.pool)
                            .await?;
                    if taken.is_some() {
                        return Err(AppError::conflict("пользователь с таким email уже существует"));
                    }
                }
                Some(e)
            }
        }
        None => None,
    };

    let country = req
        .country
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let city = req.city.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let address = req
        .address
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let avatar_url = req
        .avatar_url
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    sqlx::query(
        "UPDATE users SET \
            name          = COALESCE(NULLIF($2, ''), name), \
            gender        = $3, \
            date_of_birth = COALESCE($4, date_of_birth), \
            phone         = COALESCE(NULLIF($5, ''), phone), \
            country       = COALESCE(NULLIF($6, ''), country), \
            city          = COALESCE(NULLIF($7, ''), city), \
            address       = COALESCE(NULLIF($8, ''), address), \
            timezone      = COALESCE(NULLIF($9, ''), timezone), \
            language      = $10, \
            currency      = COALESCE(NULLIF($11, ''), currency), \
            avatar_url    = COALESCE(NULLIF($12, ''), avatar_url), \
            notify_email  = COALESCE($13, notify_email), \
            email         = COALESCE(NULLIF($14, ''), email) \
         WHERE id = $1",
    )
    .bind(claims.sub)
    .bind(name)
    .bind(gender.as_str())
    .bind(dob)
    .bind(phone)
    .bind(country)
    .bind(city)
    .bind(address)
    .bind(timezone)
    .bind(language.as_str())
    .bind(currency)
    .bind(avatar_url)
    .bind(notify_email)
    .bind(email)
    .execute(&state.pool)
    .await?;

    let profile = load_profile(&state.pool, claims.sub).await?;

    // Если изменили валюту — инвалидируем/обновляем полезную нагрузку нельзя
    // (токен несёт старую валюту). Оставляем существующий токен; при следующем
    // входе новая валюта попадёт в claims.

    Ok(Json(resp(profile)))
}

// Смена пароля: проверка текущего пароля, валидация нового, обновление хеша
// и перевыпуск токена, чтобы старая сессия не осталась «без пароля».
#[derive(Deserialize)]
pub struct PasswordReq {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Serialize)]
pub struct PasswordResp {
    pub token: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PasswordReq>,
) -> AppResult<Json<PasswordResp>> {
    let claims = require_auth(&headers, &state.jwt_secret)?;

    if req.new_password.len() < 8 {
        return Err(AppError::bad_request("новый пароль должен быть не короче 8 символов"));
    }
    if req.new_password == req.current_password {
        return Err(AppError::bad_request("новый пароль совпадает с текущим"));
    }

    let password_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(claims.sub)
        .fetch_one(&state.pool)
        .await?;

    let valid = bcrypt::verify(&req.current_password, &password_hash)
        .map_err(|e| AppError::internal(format!("ошибка проверки пароля: {e}")))?;
    if !valid {
        return Err(AppError::unauthorized("текущий пароль указан неверно"));
    }

    let new_hash = hash(&req.new_password, 12)
        .map_err(|e| AppError::internal(format!("ошибка хеширования: {e}")))?;

    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&new_hash)
        .bind(claims.sub)
        .execute(&state.pool)
        .await?;

    // Перевыпускаем токен (сессия продолжается, старый инвалидируется на практике).
    let token = common::jwt::encode_token(
        claims.sub,
        &claims.role,
        &claims.currency,
        &state.jwt_secret,
    )?;

    Ok(Json(PasswordResp { token }))
}

async fn load_profile(pool: &PgPool, user_id: i64) -> AppResult<ProfileDto> {
    Ok(sqlx::query_as::<_, ProfileDto>("\
        SELECT id, name, email, role, \
               gender, date_of_birth, phone, \
               country, city, address, timezone, language, currency, \
               avatar_url, notify_email, created_at \
        FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await?)
}

fn resp(profile: ProfileDto) -> ProfileResp {
    ProfileResp {
        languages: vec!["ru", "en"],
        genders: vec!["unspecified", "male", "female", "other"],
        profile,
        timezones: TIMEZONES.to_vec(),
        currencies: currencies::ALL.to_vec(),
    }
}