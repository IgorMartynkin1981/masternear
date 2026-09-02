#!/bin/bash
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    CREATE DATABASE auth_db;
    CREATE DATABASE catalog_db;
    CREATE DATABASE chat_db;
    CREATE DATABASE orders_db;
    CREATE DATABASE admin_db;
EOSQL