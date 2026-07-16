use crate::domain::{DatabaseColumn, DatabaseSchema, DatabaseTable};
use anyhow::{anyhow, Context};
use mysql_async::{prelude::Queryable, OptsBuilder, Pool};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use tokio::time::timeout;
use tokio_postgres::{Config, NoTls};
use tracing::info;

const DEFAULT_SCHEMA_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseConnectionInfo {
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub username: String,
    pub password: Option<String>,
}

pub async fn enrich_database_schema(
    schema: &mut DatabaseSchema,
    raw_env: &BTreeMap<String, String>,
) {
    let Some(connection) = connection_info_from_schema(schema, raw_env) else {
        return;
    };

    match introspect_schema(&connection, DEFAULT_SCHEMA_TIMEOUT).await {
        Ok(tables) => schema.tables = tables,
        Err(err) => info!(
            "database schema introspection skipped for {} on {}:{}: {}",
            connection.engine, connection.host, connection.port, err
        ),
    }
}

pub fn connection_info_from_schema(
    schema: &DatabaseSchema,
    raw_env: &BTreeMap<String, String>,
) -> Option<DatabaseConnectionInfo> {
    let engine = normalize_engine(&schema.engine)?;
    let parsed_url = database_url(raw_env).and_then(parse_database_url);
    let default_port = if engine == "mysql" { 3306 } else { 5432 };

    let host = env_value(
        raw_env,
        if engine == "mysql" {
            &["MYSQL_HOST", "DB_HOST"]
        } else {
            &["POSTGRES_HOST", "PGHOST", "DB_HOST"]
        },
    )
    .map(str::to_string)
    .or_else(|| schema.host.clone())
    .or_else(|| parsed_url.as_ref().and_then(|url| url.host.clone()))?;

    let port = env_value(
        raw_env,
        if engine == "mysql" {
            &["MYSQL_PORT", "DB_PORT"]
        } else {
            &["POSTGRES_PORT", "PGPORT", "DB_PORT"]
        },
    )
    .and_then(|value| value.parse::<u16>().ok())
    .or_else(|| schema.port.and_then(|value| u16::try_from(value).ok()))
    .or_else(|| parsed_url.as_ref().and_then(|url| url.port))
    .unwrap_or(default_port);

    let database_name = env_value(
        raw_env,
        if engine == "mysql" {
            &["MYSQL_DATABASE", "DB_NAME"]
        } else {
            &["POSTGRES_DB", "PGDATABASE", "DB_NAME"]
        },
    )
    .map(str::to_string)
    .or_else(|| schema.database_name.clone())
    .or_else(|| {
        parsed_url
            .as_ref()
            .and_then(|url| url.database_name.clone())
    })?;

    let username = env_value(
        raw_env,
        if engine == "mysql" {
            &["MYSQL_USER", "MYSQL_ROOT_USER", "DB_USER"]
        } else {
            &["POSTGRES_USER", "PGUSER", "DB_USER"]
        },
    )
    .map(str::to_string)
    .or_else(|| schema.username.clone())
    .or_else(|| parsed_url.as_ref().and_then(|url| url.username.clone()))?;

    let password = env_value(
        raw_env,
        if engine == "mysql" {
            &["MYSQL_PASSWORD", "MYSQL_ROOT_PASSWORD", "DB_PASSWORD"]
        } else {
            &["POSTGRES_PASSWORD", "PGPASSWORD", "DB_PASSWORD"]
        },
    )
    .filter(|value| *value != crate::extractor::REDACTED_ENV_VALUE)
    .map(str::to_string)
    .or_else(|| parsed_url.and_then(|url| url.password));

    Some(DatabaseConnectionInfo {
        engine,
        host,
        port,
        database_name,
        username,
        password,
    })
}

pub async fn introspect_schema(
    connection: &DatabaseConnectionInfo,
    timeout_duration: Duration,
) -> anyhow::Result<Vec<DatabaseTable>> {
    let result = if connection.engine == "mysql" {
        timeout(timeout_duration, introspect_mysql(connection)).await
    } else {
        timeout(timeout_duration, introspect_postgres(connection)).await
    };

    match result {
        Ok(result) => result,
        Err(_) => Err(anyhow!("timed out after {}s", timeout_duration.as_secs())),
    }
}

async fn introspect_postgres(
    connection: &DatabaseConnectionInfo,
) -> anyhow::Result<Vec<DatabaseTable>> {
    let mut config = Config::new();
    config
        .host(&connection.host)
        .port(connection.port)
        .user(&connection.username)
        .dbname(&connection.database_name)
        .connect_timeout(DEFAULT_SCHEMA_TIMEOUT);
    if let Some(password) = &connection.password {
        config.password(password);
    }

    let (client, connection_task) = config
        .connect(NoTls)
        .await
        .with_context(|| "connect to postgres")?;
    tokio::spawn(async move {
        if let Err(err) = connection_task.await {
            info!("postgres schema introspection connection closed: {}", err);
        }
    });

    client
        .batch_execute("SET default_transaction_read_only = on; SET statement_timeout = '5000ms'; BEGIN READ ONLY;")
        .await
        .with_context(|| "start postgres read-only session")?;

    let rows = client
        .query(
            "SELECT table_name, column_name, data_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
             ORDER BY table_schema, table_name, ordinal_position",
            &[],
        )
        .await
        .with_context(|| "query postgres information_schema.columns")?;
    let _ = client.batch_execute("ROLLBACK").await;

    let primary_keys = postgres_primary_keys(&client).await.unwrap_or_default();
    let columns = rows.into_iter().map(|row| ColumnRow {
        table_name: row.get::<_, String>(0),
        column_name: row.get::<_, String>(1),
        data_type: row.get::<_, String>(2),
        is_nullable: row.get::<_, String>(3) == "YES",
        default_value: row.get::<_, Option<String>>(4),
    });

    Ok(tables_from_columns(columns, &primary_keys))
}

async fn postgres_primary_keys(
    client: &tokio_postgres::Client,
) -> anyhow::Result<BTreeSet<(String, String)>> {
    let rows = client
        .query(
            "SELECT kcu.table_name, kcu.column_name \
             FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu \
               ON tc.constraint_name = kcu.constraint_name \
              AND tc.table_schema = kcu.table_schema \
             WHERE tc.constraint_type = 'PRIMARY KEY' \
               AND tc.table_schema NOT IN ('pg_catalog', 'information_schema')",
            &[],
        )
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
        .collect())
}

async fn introspect_mysql(
    connection: &DatabaseConnectionInfo,
) -> anyhow::Result<Vec<DatabaseTable>> {
    let mut builder = OptsBuilder::default()
        .ip_or_hostname(connection.host.clone())
        .tcp_port(connection.port)
        .user(Some(connection.username.clone()))
        .db_name(Some(connection.database_name.clone()));
    if let Some(password) = &connection.password {
        builder = builder.pass(Some(password.clone()));
    }

    let pool = Pool::new(builder);
    let mut conn = pool.get_conn().await.with_context(|| "connect to mysql")?;
    conn.query_drop("SET SESSION TRANSACTION READ ONLY")
        .await
        .with_context(|| "set mysql transaction read-only")?;
    conn.query_drop("START TRANSACTION READ ONLY")
        .await
        .with_context(|| "start mysql read-only transaction")?;

    let columns: Vec<ColumnRow> = conn
        .exec_map(
            "SELECT table_name, column_name, data_type, is_nullable, column_default \
             FROM information_schema.columns \
             WHERE table_schema = ? \
             ORDER BY table_name, ordinal_position",
            (connection.database_name.clone(),),
            |(table_name, column_name, data_type, is_nullable, default_value): (
                String,
                String,
                String,
                String,
                Option<String>,
            )| ColumnRow {
                table_name,
                column_name,
                data_type,
                is_nullable: is_nullable == "YES",
                default_value,
            },
        )
        .await
        .with_context(|| "query mysql information_schema.columns")?;

    let primary_keys = conn
        .exec_map(
            "SELECT table_name, column_name \
             FROM information_schema.key_column_usage \
             WHERE table_schema = ? AND constraint_name = 'PRIMARY'",
            (connection.database_name.clone(),),
            |(table_name, column_name): (String, String)| (table_name, column_name),
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let _ = conn.query_drop("ROLLBACK").await;
    pool.disconnect().await.ok();

    Ok(tables_from_columns(columns.into_iter(), &primary_keys))
}

#[derive(Debug)]
struct ColumnRow {
    table_name: String,
    column_name: String,
    data_type: String,
    is_nullable: bool,
    default_value: Option<String>,
}

fn tables_from_columns(
    columns: impl Iterator<Item = ColumnRow>,
    primary_keys: &BTreeSet<(String, String)>,
) -> Vec<DatabaseTable> {
    let mut tables = BTreeMap::<String, DatabaseTable>::new();
    for column in columns {
        let table = tables
            .entry(column.table_name.clone())
            .or_insert_with(|| DatabaseTable {
                name: column.table_name.clone(),
                ..DatabaseTable::default()
            });
        table.columns.push(DatabaseColumn {
            name: column.column_name.clone(),
            data_type: column.data_type,
            nullable: column.is_nullable,
            primary_key: primary_keys.contains(&(column.table_name, column.column_name)),
            default_value: column.default_value,
        });
    }
    tables.into_values().collect()
}

fn normalize_engine(engine: &str) -> Option<String> {
    let normalized = engine.to_ascii_lowercase();
    match normalized.as_str() {
        "postgres" | "postgresql" | "postgis" => Some("postgresql".to_string()),
        "mysql" | "mariadb" => Some("mysql".to_string()),
        _ => None,
    }
}

fn env_value<'a>(env: &'a BTreeMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| env.get(*key).map(String::as_str))
}

fn database_url(env: &BTreeMap<String, String>) -> Option<&str> {
    env_value(
        env,
        &[
            "DATABASE_URL",
            "POSTGRES_URL",
            "POSTGRESQL_URL",
            "MYSQL_URL",
            "MARIADB_URL",
        ],
    )
}

#[derive(Debug, Default)]
struct ParsedDatabaseUrl {
    host: Option<String>,
    port: Option<u16>,
    database_name: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

fn parse_database_url(value: &str) -> Option<ParsedDatabaseUrl> {
    let (scheme, rest) = value.split_once("://")?;
    normalize_engine(scheme)?;
    let without_query = rest.split(['?', '#']).next().unwrap_or(rest);
    let (credentials, host_path) = without_query
        .rsplit_once('@')
        .map(|(credentials, host_path)| (Some(credentials), host_path))
        .unwrap_or((None, without_query));
    let (host_port, database_name) = host_path
        .split_once('/')
        .map(|(host_port, database)| (host_port, non_empty(database)))
        .unwrap_or((host_path, None));
    let (host, port) = host_port
        .rsplit_once(':')
        .map(|(host, port)| (non_empty(host), port.parse::<u16>().ok()))
        .unwrap_or((non_empty(host_port), None));
    let (username, password) = credentials
        .and_then(|value| {
            value
                .split_once(':')
                .map(|(user, password)| (user, Some(password)))
                .or(Some((value, None)))
        })
        .map(|(user, password)| {
            (
                non_empty(user).map(str::to_string),
                password.and_then(non_empty).map(str::to_string),
            )
        })
        .unwrap_or((None, None));

    Some(ParsedDatabaseUrl {
        host: host.map(str::to_string),
        port,
        database_name: database_name.map(str::to_string),
        username,
        password,
    })
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_postgres_connection_info_from_raw_env() {
        let schema = DatabaseSchema {
            engine: "postgresql".to_string(),
            host: None,
            port: None,
            database_name: None,
            username: None,
            source_container_id: "c1".to_string(),
            source_container_name: "api".to_string(),
            tables: Vec::new(),
        };
        let raw_env = BTreeMap::from([
            (
                "DATABASE_URL".to_string(),
                "postgres://app:secret@db:5432/appdb".to_string(),
            ),
            ("POSTGRES_PASSWORD".to_string(), "env-secret".to_string()),
        ]);

        let connection = connection_info_from_schema(&schema, &raw_env).unwrap();

        assert_eq!(connection.engine, "postgresql");
        assert_eq!(connection.host, "db");
        assert_eq!(connection.port, 5432);
        assert_eq!(connection.database_name, "appdb");
        assert_eq!(connection.username, "app");
        assert_eq!(connection.password.as_deref(), Some("env-secret"));
    }

    #[test]
    fn maps_column_rows_to_tables() {
        let primary_keys = BTreeSet::from([("users".to_string(), "id".to_string())]);
        let tables = tables_from_columns(
            vec![
                ColumnRow {
                    table_name: "users".to_string(),
                    column_name: "id".to_string(),
                    data_type: "integer".to_string(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnRow {
                    table_name: "users".to_string(),
                    column_name: "email".to_string(),
                    data_type: "text".to_string(),
                    is_nullable: false,
                    default_value: None,
                },
            ]
            .into_iter(),
            &primary_keys,
        );

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "users");
        assert_eq!(tables[0].columns[0].name, "id");
        assert!(tables[0].columns[0].primary_key);
        assert_eq!(tables[0].columns[1].data_type, "text");
    }
}
