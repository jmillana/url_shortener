use md5;
use serde::{Deserialize, Serialize};
use warp::{
    http::{StatusCode, Uri},
    Filter,
};

extern crate dotenv;
extern crate mysql;

use dotenv::dotenv;
use mysql::prelude::*;
use std::{convert::Infallible, env, fs};

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

async fn shorten_url(
    db_pool: mysql::Pool,
    req: ShortenUrlPostRequest,
) -> Result<impl warp::Reply, warp::Rejection> {
    // Check if the url is already on the db
    println!("URL: {:?}", req);
    let url = req.url;
    let (short, exists_in_db) = select_hash(&db_pool, url.clone());
    let shorten = ShortenUrl { url, short };
    if !exists_in_db {
        add_to_db(&db_pool, shorten.clone()).expect("Unable to update the database");
    }
    return Ok(warp::reply::json(&shorten));
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrl {
    pub url: String,
    pub short: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrlPostRequest {
    pub url: String,
}
fn json_body() -> impl Filter<Extract = (ShortenUrlPostRequest,), Error = warp::Rejection> + Clone {
    // When accepting a body, we want a JSON body
    // (and to reject huge payloads)...
    warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}

fn with_db(
    db_pool: mysql::Pool,
) -> impl Filter<Extract = (mysql::Pool,), Error = Infallible> + Clone {
    warp::any().map(move || db_pool.clone())
}

async fn retrieve_url(
    db_pool: mysql::Pool,
    hash: String,
) -> Result<impl warp::Reply, warp::Rejection> {
    println!("Checking hash: {}", hash);
    let item = retrieve_hash_from_db(&db_pool, hash);
    match item {
        Some(item) => {
            return Ok(warp::redirect(item.url.parse::<Uri>().unwrap()));
        }
        None => {
            println!("Send NOT FOUND error");
            //Reject
            return Err(warp::reject::not_found());
        }
    }
}

async fn handle_rejection(err: warp::Rejection) -> Result<impl warp::Reply, warp::Rejection> {
    if err.is_not_found() {
        Ok(warp::reply::with_status("NOT_FOUND", StatusCode::NOT_FOUND))
    } else {
        return Err(err);
    }
}

async fn mask_termination_error(
    err: warp::Rejection,
) -> Result<impl warp::Reply, std::convert::Infallible> {
    eprintln!("unhandled rejection: {:?}", err);
    Ok(warp::reply::with_status(
        "INTERNAL_SERVER_ERROR",
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

#[tokio::main]
async fn main() {
    // Load environment variables form .env file
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL not set");

    let db_builder = mysql::OptsBuilder::from_opts(mysql::Opts::from_url(&database_url).unwrap());
    let db_pool = mysql::Pool::new(db_builder.ssl_opts(mysql::SslOpts::default())).unwrap();

    init_db(&db_pool).await.expect("Unable to init db");
    // GET /hello/warp => 200 OK with body "Hello, warp!"
    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));

    let get_url = warp::get()
        .and(with_db(db_pool.clone()))
        .and(warp::path::param())
        .and_then(retrieve_url)
        .recover(handle_rejection);

    let shorten_url = warp::post()
        .and(warp::path("v1"))
        .and(warp::path("shorten"))
        .and(warp::path::end())
        .and(with_db(db_pool.clone()))
        .and(json_body())
        .and_then(shorten_url)
        .recover(handle_rejection);

    let routes = shorten_url
        .or(hello)
        .or(get_url)
        .recover(mask_termination_error);
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await
}
