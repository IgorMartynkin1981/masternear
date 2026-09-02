use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth_pool: PgPool,
    pub catalog_pool: PgPool,
    pub orders_pool: PgPool,
    pub jwt_secret: String,
}