use std::path::Path;

use dotenv::dotenv;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue};
use worker::*;

// use md5;
// use mysql::prelude::*;

use askama::Template;

mod utils;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrl {
    pub url: String,
    pub short: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrlPostRequest {
    pub url: String,
}

fn log_request(req: &Request) {
    console_log!(
        "{} - [{}], located at: {:?}, within: {}",
        Date::now().to_string(),
        req.path(),
        req.cf().coordinates().unwrap_or_default(),
        req.cf().region().unwrap_or("unknown region".into())
    );
}

pub fn shorten(url: String) -> String {
    let digest = md5::compute(url);
    return format!("{:x}", digest);
}

// pub async fn init_db(db_pool: &mysql::Pool) -> std::result::Result<(), Box<dyn std::error::Error>> {
//     // let init_file = fs::read_to_string(INIT_SQL)?;
//     let mut conn = db_pool
//         .get_conn()
//         .expect("Failed to get connection from the pool");
//     conn.query_drop(INIT_SQL_FILE)?;
//     println!("DB initialized: OK");
//     return Ok(());
// }

pub async fn retrieve_hash_from_db(hash: String, d1: &D1Database) -> Option<ShortenUrl> {
    println!("Retrieve data from DB");
    let statement = d1.prepare("SELECT short, url FROM urls WHERE short = ?1");
    let query = statement.bind(&[JsValue::from_str(hash.as_str())]).unwrap();
    let results = query.first::<ShortenUrl>(None).await;
    return results.unwrap();
}
//
pub async fn add_to_db(
    shorten: ShortenUrl,
    d1: &D1Database,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let url = JsValue::from_str(shorten.url.as_str());
    let hash = JsValue::from_str(shorten.short.as_str());
    let statement = d1.prepare("INSERT INTO urls (short, url) VALUES (?1, ?2)");
    let query = statement.bind(&[url, hash]).unwrap();
    match query.run().await {
        Ok(_) => {
            console_log!(
                "Successfully added {}:{} to the DB",
                shorten.short,
                shorten.url
            );
            return Ok(());
        }
        Err(_) => {
            console_log!("Failed to add {}:{} to the DB", shorten.short, shorten.url);
            return Err("Failed to add to the DB".into());
        }
    }
}

async fn select_hash(url: String, d1: &D1Database) -> (String, bool) {
    let mut found = false;
    let mut exists = false;
    let digest = shorten(url.clone());
    let mut count = 0;
    let mut hash = digest[count..count + 8].to_string();
    while !found && count < digest.len() - 8 {
        hash = digest[count..count + 8].to_string();

        let response = retrieve_hash_from_db(hash.clone(), d1).await;

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

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    log_request(&req);
    dotenv().ok();

    // Optionally, get more helpful error messages written to the console in the case of a panic.
    utils::set_panic_hook();
    console_log!("LOAD OK");

    // Optionally, use the Router to handle matching endpoints, use ":name" placeholders, or "*name"
    // catch-alls to match on specific patterns. Alternatively, use `Router::with_data(D)` to
    // provide arbitrary data that will be accessible in each route via the `ctx.data()` method.
    let router = Router::new();

    // Add as many routes as your Worker needs! Each route will get a `Request` for handling HTTP
    // functionality and a `RouteContext` which you can use to  and get route parameters and
    // Environment bindings like KV Stores, Durable Objects, Secrets, and Variables.
    router
        .get_async("/", handle_home)
        .get_async("/assets/:file", handle_assets)
        .get_async("/:hash", handle_get_url)
        .post_async("/api/shorten", handle_post_url)
        .run(req, env)
        .await
}

async fn handle_post_url(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    // let hash = match ctx.param("hash") {
    //     Some(hash) => hash,
    //     None => return Response::error("Bad request", 400),
    // };
    console_log!("AT post URL");
    let data = req.json::<ShortenUrlPostRequest>().await?;
    let url = data.url;

    let d1 = extract_db(&ctx);
    let (short, exists_in_db) = select_hash(url.clone(), &d1).await;
    let shorten = ShortenUrl {
        url,
        short: short.clone(),
    };
    if !exists_in_db {
        match add_to_db(shorten.clone(), &d1).await {
            Ok(_) => console_log!("Hash {} inserted to the DB", short),
            Err(_) => return Response::error(format!("Unable to register the hash"), 500),
        }
    }
    return Response::from_json(&json!(shorten));
}

fn extract_db(ctx: &RouteContext<()>) -> D1Database {
    return ctx.env.d1("DB").expect("Unable to access the D1 database");
}

async fn handle_get_url(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let hash = match ctx.param("hash") {
        Some(hash) => hash,
        None => return Response::error("Bad request", 400),
    };
    let d1 = extract_db(&ctx);
    match retrieve_hash_from_db(hash.clone(), &d1).await {
        Some(item) => {
            let url = Url::parse(item.url.as_str())?;
            return Response::redirect(url);
        }
        None => {
            println!("Send NOT FOUND error");
            return Response::error(format!("URL for hash {} not found", hash), 400);
        }
    }
}

async fn handle_home(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let template = HelloTemplate {};
    match template.render() {
        Ok(html) => return Response::from_html(html),
        Err(_) => return Response::error("Bad Request", 400),
    };
}

async fn handle_assets(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let file = match ctx.param("file") {
        Some(file) => file,
        None => return Response::error("Bad request", 400),
    };
    let kv = ctx.kv("ASSETS")?;
    let path = Path::new(file);
    let content_type = ContentType::from_ext(path.extension().unwrap().to_str().unwrap());

    console_log!("File: {}, content type: {}", file, content_type.to_str());
    let (value, _) = kv.get(file).bytes_with_metadata::<Vec<u8>>().await?;
    match value {
        Some(value) => {
            let resp = Response::from_bytes(value)?;
            let mut headers = Headers::new();
            let _ = headers.append("Content-Type", content_type.to_str())?;
            return Ok(resp.with_headers(headers));
        }
        None => return Response::error(format!("File {} not found", file), 404),
    }
}

enum ContentType {
    CSS,
    HTML,
    PNG,
    TXT,
}

impl ContentType {
    fn from_ext(ext: &str) -> ContentType {
        return match ext {
            "css" => ContentType::CSS,
            "html" => ContentType::HTML,
            "png" => ContentType::PNG,
            _ => ContentType::TXT,
        };
    }
    fn to_str(&self) -> &str {
        return match *self {
            ContentType::CSS => "text/css",
            ContentType::HTML => "text/html",
            ContentType::PNG => "image/png",
            ContentType::TXT => "text/plain",
        };
    }
}

#[derive(Template)]
#[template(path = "hello.html")]
struct HelloTemplate;
