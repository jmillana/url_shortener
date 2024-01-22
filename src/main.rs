use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use md5;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
extern crate dotenv;
extern crate mysql;

use dotenv::dotenv;
use mysql::prelude::*;
use serde::{Deserialize, Serialize};
use std::{env, fs};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrl {
    pub url: String,
    pub short: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrlPostRequest {
    pub url: String,
}

const INIT_SQL: &str = "./db.sql";

pub fn shorten(url: String) -> String {
    let digest = md5::compute(url);
    return format!("{:x}", digest);
}

pub async fn init_db(db_pool: &mysql::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let init_file = fs::read_to_string(INIT_SQL)?;
    let mut conn = db_pool
        .get_conn()
        .expect("Failed to get connection from the pool");
    conn.query_drop(init_file.as_str())?;
    println!("DB initialized: OK");
    return Ok(());
}

pub fn retrieve_hash_from_db(db_pool: &mysql::Pool, hash: String) -> Option<ShortenUrl> {
    println!("Retrieve data from DB");
    let mut conn = db_pool
        .get_conn()
        .expect("Failed to get connection from the pool");

    let short_urls = conn
        .query_map(
            format!("SELECT short, url FROM urls WHERE short='{}'", hash),
            |(short_url, url)| ShortenUrl {
                short: short_url,
                url,
            },
        )
        .expect("Failed to execute query");
    if short_urls.len() == 0 {
        println!("Hash not found {}", hash);
        return None;
    } else if short_urls.len() == 1 {
        println!("Hash {} already present on the DB", hash);
        return Some(short_urls[0].clone());
    } else {
        println!("More that one match found for hash: {}", hash);
        unreachable!("Shouldn't be more than one match on the DB");
    }
}

pub fn add_to_db(
    db_pool: &mysql::Pool,
    shorten: ShortenUrl,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = db_pool
        .get_conn()
        .expect("Failed to get connection from the pool");

    conn.exec_drop(
        r"INSERT INTO urls (short, url)
          VALUES (:short, :url)",
        (shorten.short, shorten.url),
    )?;
    println!("Item added to DB");
    return Ok(());
}

fn select_hash(db_pool: &mysql::Pool, url: String) -> (String, bool) {
    let mut found = false;
    let mut exists = false;
    let digest = shorten(url.clone());
    let mut count = 0;
    let mut hash = digest[count..count + 8].to_string();
    while !found && count < digest.len() - 8 {
        hash = digest[count..count + 8].to_string();
        let response = retrieve_hash_from_db(&db_pool, hash.clone());

        match response {
            Some(shorten) => {
                if shorten.url == url {
                    println!("Provided URL matches the HASH");
                    exists = true;
                    found = true;
                } else {
                    println!("HASH collision");
                    count += 1;
                }
            }
            None => {
                println!("Found a new HASH for the given URL");
                found = true;
            }
        };
    }
    if !found {
        panic!("Unable to find a hash for the provided URL");
    }
    return (hash, exists);
}

pub async fn url_post_handler(
    State(data): State<Arc<AppState>>,
    Json(body): Json<ShortenUrlPostRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    print!("At url handler");
    let (short, exists_in_db) = select_hash(&data.db, body.url.to_string());
    let shorten = ShortenUrl {
        url: body.url.to_string(),
        short,
    };
    if !exists_in_db {
        add_to_db(&data.db, shorten.clone()).expect("Unable to update the database");
    }
    let response = serde_json::json!({
        "status": "success", "data": serde_json::json!(shorten)
    });
    return Ok((StatusCode::CREATED, Json(response)));
}

async fn short_url_get_handler(
    State(data): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    println!("Checking hash: {}", hash);
    let item = retrieve_hash_from_db(&data.db, hash.clone());
    match item {
        Some(item) => {
            let redirect = Redirect::to(item.url.as_str());
            return Ok(redirect.into_response());
        }
        None => {
            println!("Send NOT FOUND error");
            let error_msg = serde_json::json!({
                "status": "fail",
                "message": format!("URL with short tag: {} not found", hash)
            });
            return Err((StatusCode::NOT_FOUND, axum::Json(error_msg)));
        }
    }
}

pub struct AppState {
    db: mysql::Pool,
}

pub fn create_router(app_state: Arc<AppState>) -> Router {
    let serve_dir =
        ServeDir::new("assets").not_found_service(ServeFile::new("templates/index.html"));
    return Router::new()
        .route("/v1/shorten", post(url_post_handler))
        .route("/:short", get(short_url_get_handler))
        .with_state(app_state);
}

#[tokio::main]
async fn main() {
    // Load environment variables form .env file
    dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "url-shortener=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL not set");

    let db_builder = mysql::OptsBuilder::from_opts(mysql::Opts::from_url(&database_url).unwrap());
    let db_pool = mysql::Pool::new(db_builder.ssl_opts(mysql::SslOpts::default())).unwrap();

    init_db(&db_pool).await.expect("Unable to init db");
    let app_state = Arc::new(AppState {
        db: db_pool.clone(),
    });

    let app = create_router(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
}
