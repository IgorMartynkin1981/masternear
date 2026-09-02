# MasterNear

Бэкенд-платформа для поиска мастера на дом. Заказчик находит мастера по категории работ (сантехника, электрика и т.д.), видит его ценники, публикует заказ с ценником и выбирает мастера из откликнувшихся, а также общается в чате. Есть веб-сайт (SPA), который раздаёт API-шлюз.

## Веб-сайт

Современный SPA (тёмная тема, glassmorphism) — открывается на `http://127.0.0.1:8080`:

- **Лендинг** — hero-блок, преимущества сервиса
- **Каталог мастеров** — фильтр по категориям, карточки с рейтингом (1–5 ★) и ценниками, кнопка «Написать мастеру»
- **Заказы** — заказчик публикует работу с ценником «сколько готов заплатить»; мастера видят новые заказы (счётчик в меню) и борются за них, предлагая свою цену; заказчик выбирает любого мастера
- **Авторизация** — вход/регистрация (роль: заказчик или мастер), сессия на JWT в `localStorage`
- **Чаты** — список диалогов, переписка в обе стороны с автообновлением (polling 6 c)
- **Панель мастера** — анкета (имя, о себе) и управление ценниками по категориям
- **Обратная связь** (`#/feedback`) — форма для всех пользователей, обращения попадают в админку
- **Админ-панель** (`#/admin`) — доступна администраторам: обзор статистики, просмотр/создание/изменение/удаление пользователей, заказов, обработка обращений

Учётка администратора: `admin@example.com` / `admin123` (создаётся автоматически при старте `admin-service`).

Статика лежит в `crates/gateway-service/static/` (изменения — достаточно пересобрать gateway).

## Архитектура (микросервисы)

```
┌────────────┐   /api/auth/*   ┌────────────────┐
│            │ ──────────────▶ │ auth-service   │  регистрация, логин, JWT (Postgres: auth_db)
│  gateway   │   /api/categories, /api/masters/*  │
│  (8080)    │ ──────────────▶ │ catalog-service│  мастера, категории, ценники, рейтинги (Postgres: catalog_db)
│  + САЙТ    │   /api/chats/*  ┌────────────────┐
│            │ ──────────────▶ │ chat-service   │  чат заказчик-мастер (Postgres: chat_db)
│            │   /api/orders/* ┌────────────────┐
│            │ ──────────────▶ │ order-service  │  заказы и торги мастеров (Postgres: orders_db)
│            │  /api/admin/*   ┌────────────────┐
│            │ ──────────────▶ │ admin-service  │  админ-панель + обратная связь (Postgres: admin_db)
└────────────┘                 └────────────────┘
```

Каждый сервис — отдельный Cargo-крейт со своей БД и Docker-образом. Перекрёстное общение через API-шлюз, авторизация — JWT (HS256, общий секрет `JWT_SECRET`, TTL 24ч).

```
Cargo.toml                    // workspace
crates/
├── common/                   // общая библиотека: JWT, ошибки API
├── auth-service/             // порт 8081
├── catalog-service/          // порт 8082
├── chat-service/             // порт 8083
├── order-service/            // порт 8084
├── admin-service/            // порт 8085
└── gateway-service/          // порт 8080
```

## Запуск

```bash
docker compose up -d --build     # Postgres на хосте: 127.0.0.1:5433
```

Все сервисы поднимаются автоматически. Наружу доступен только gateway `http://127.0.0.1:8080`.

Локально (без Docker): нужен запущенный Postgres, в каждой БД применить схему автоматически:
```bash
DATABASE_URL=postgres://.../auth_db JWT_SECRET=... cargo run -p auth-service
```

## API (через gateway `:8080`)

### Авторизация
| Метод | Путь | Описание |
|---|---|---|
| POST | `/api/auth/register` | `{name, email, password, role}` где role: `customer` / `master` → `{token, user}` |
| POST | `/api/auth/login` | `{email, password}` → `{token, user}` |
| GET | `/api/auth/me` | профиль текущего пользователя |

### Каталог мастеров
| Метод | Путь | Описание |
|---|---|---|
| GET | `/api/categories` | список категорий работ |
| GET | `/api/masters?category_id=` | мастера (с ценниками), фильтр по категории |
| POST | `/api/masters/me` | создать/обновить профиль мастера `{name, bio}` (мастер) |
| GET | `/api/masters/me` | свой профиль с ценниками |
| PUT | `/api/masters/me/prices` | установить ценник `{category_id, price}` (мастер) |
| POST | `/api/masters/{user_id}/rating` | оценить мастера `{score: 1..5}` (заказчик) → `{avg, count, my_score}` |
| GET | `/api/masters` | мастера с полями `rating` (среднее) и `rating_count` |

### Чат
| Метод | Путь | Описание |
|---|---|---|
| POST | `/api/chats` | создать чат с мастером `{master_id, first_message?}` (заказчик) |
| GET | `/api/chats` | список моих чатов |
| GET | `/api/chats/{id}/messages` | переписка (только участники) |
| POST | `/api/chats/{id}/messages` | отправить сообщение `{text}` |

### Заказы и торги
| Метод | Путь | Описание |
|---|---|---|
| POST | `/api/orders` | создать заказ `{category_id, title, description?, budget}` (заказчик) |
| GET | `/api/orders` | заказчик — свои заказы с предложениями; мастер — открытые заказы + его ставки |
| GET | `/api/orders/{id}` | детали заказа (владелец или мастер) |
| POST | `/api/orders/{id}/offers` | предложить цену `{price, comment?}` (мастер, только пока заказ открыт) |
| POST | `/api/orders/{id}/select` | выбрать предложение `{offer_id}` (владелец заказа) |

### Обратная связь и админка
| Метод | Путь | Описание |
|---|---|---|
| POST | `/api/feedback` | отправить обращение `{message, email?}` (все) |
| GET | `/api/admin/overview` | статистика (админ) |
| GET/POST/PUT/DELETE | `/api/admin/users` | управление пользователями (админ) |
| GET/PUT/DELETE | `/api/admin/orders[/{id}]` | просмотр и правка заказов (админ) |
| GET | `/api/admin/feedback` | список обращений (админ) |
| PATCH | `/api/admin/feedback/{id}` | отметить обращение выполненным (админ) |

## Пример полного сценария

```bash
# мастер
MT=$(curl -s -X POST localhost:8080/api/auth/register -H 'Content-Type: application/json' \
  -d '{"name":"Пётр","email":"peter@mail.ru","password":"secret123","role":"master"}' \
  | jq -r .token)
curl -X POST localhost:8080/api/masters/me -H "Authorization: Bearer $MT" \
  -H 'Content-Type: application/json' -d '{"name":"Пётр","bio":"Сантехник"}'
curl -X PUT localhost:8080/api/masters/me/prices -H "Authorization: Bearer $MT" \
  -H 'Content-Type: application/json' -d '{"category_id":1,"price":35}'

# заказчик
CT=$(curl -s -X POST localhost:8080/api/auth/register -H 'Content-Type: application/json' \
  -d '{"name":"Игорь","email":"igor@mail.ru","password":"secret123","role":"customer"}' \
  | jq -r .token)
curl -s "localhost:8080/api/masters?category_id=1"
# оценка мастера 5 звёзд (один клиент может изменить свою оценку)
curl -X POST localhost:8080/api/masters/1/rating -H "Authorization: Bearer $CT" \
  -H 'Content-Type: application/json' -d '{"score":5}'
# заказчик публикует заказ с ценником
curl -X POST localhost:8080/api/orders -H "Authorization: Bearer $CT" \
  -H 'Content-Type: application/json' \
  -d '{"category_id":1,"title":"Починить кран","description":"Течёт смеситель","budget":3000}'
# мастер видит новый заказ и предлагает свою цену
curl -X POST localhost:8080/api/orders/1/offers -H "Authorization: Bearer $MT" \
  -H 'Content-Type: application/json' -d '{"price":2500,"comment":"Приеду завтра"}'
# заказчик выбирает мастера
curl -X POST localhost:8080/api/orders/1/select -H "Authorization: Bearer $CT" \
  -H 'Content-Type: application/json' -d '{"offer_id":1}'
CID=$(curl -s -X POST localhost:8080/api/chats -H "Authorization: Bearer $CT" \
  -H 'Content-Type: application/json' -d '{"master_id":1,"first_message":"Почините кран"}' | jq -r .id)
curl -X POST localhost:8080/api/chats/$CID/messages -H "Authorization: Bearer $CT" \
  -H 'Content-Type: application/json' -d '{"text":"Когда сможете приехать?"}'
```

## Переменные окружения

| Сервис | Переменные |
|---|---|
| все | `JWT_SECRET`, `SERVER_ADDR` |
| auth | `DATABASE_URL` |
| catalog | `DATABASE_URL` |
| chat | `DATABASE_URL` |
| gateway | `SERVER_ADDR`, `AUTH_SERVICE_URL`, `CATALOG_SERVICE_URL`, `CHAT_SERVICE_URL` |

## Технологии

Rust + axum + sqlx, PostgreSQL 16, Docker Compose, JWT (jsonwebtoken), bcrypt.