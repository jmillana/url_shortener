use std::path::Path;

use dotenv::dotenv;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen::JsValue;
use worker::*;

use md5;

use askama::Template;

mod utils;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrl {
    pub url: String,
    pub slug: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ShortenUrlPostRequest {
    pub url: String,
    pub slug: Option<String>,
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

pub async fn retrieve_slug_from_db(slug: String, d1: &D1Database) -> Option<ShortenUrl> {
    console_log!("Retrieve data from DB");
    let statement = d1.prepare("SELECT slug, url FROM urls WHERE slug = ?1");
    let query = statement.bind(&[JsValue::from_str(slug.as_str())]).unwrap();
    let results = query.first::<ShortenUrl>(None).await;
    return results.unwrap();
}

pub async fn add_to_db(
    shorten: ShortenUrl,
    d1: &D1Database,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let url = JsValue::from_str(shorten.url.as_str());
    let slug = JsValue::from_str(shorten.slug.as_str());
    let statement = d1.prepare("INSERT INTO urls (slug, url) VALUES (?1, ?2)");
    let query = statement.bind(&[slug, url]).unwrap();
    match query.run().await {
        Ok(_) => {
            console_log!(
                "Successfully added {}:{} to the DB",
                shorten.slug,
                shorten.url
            );
            return Ok(());
        }
        Err(_) => {
            console_log!("Failed to add {}:{} to the DB", shorten.slug, shorten.url);
            return Err("Failed to add to the DB".into());
        }
    }
}

async fn generate_slug(url: String, d1: &D1Database) -> (String, bool) {
    let mut found = false;
    let mut exists = false;
    let digest = shorten(url.clone());
    let mut count = 0;
    let mut slug = digest[count..count + 8].to_string();
    while !found && count < digest.len() - 8 {
        slug = digest[count..count + 8].to_string();

        let response = retrieve_slug_from_db(slug.clone(), d1).await;
        console_log!("Resp: {:?}", response);

        match response {
            Some(shorten) => {
                if shorten.url == url {
                    console_log!("Provided URL matches the Slug");
                    exists = true;
                    found = true;
                } else {
                    console_log!("Slug collision");
                    count += 1;
                }
            }
            None => {
                console_log!("Found a new Slug for the given URL");
                found = true;
            }
        };
    }
    if !found {
        panic!("Unable to find a slug for the provided URL");
    }
    return (slug, exists);
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
        .get_async("/:slug", handle_get_url)
        .post_async("/api/shorten", handle_post_url)
        .run(req, env)
        .await
}

async fn handle_post_url(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let data = req.json::<ShortenUrlPostRequest>().await?;
    let url = data.url;
    let d1 = extract_db(&ctx);
    let (slug, is_slug_in_db) = match data.slug {
        // Check if the slug is already in the DB
        //
        // If the slug is already on the DB check the URL
        //  - If is the same as the provided one: continue as expected
        //  - If the URL is different: Return an error response
        Some(mut slug) => {
            let is_slug_in_db;
            if slug.len() == 0 {
                (slug, is_slug_in_db) = generate_slug(url.clone(), &d1).await;
            } else {
                let response = retrieve_slug_from_db(slug.clone(), &d1).await;
                console_log!("Resp: {:?}", response);

                is_slug_in_db = match response {
                    Some(item) => {
                        if item.url == url {
                            console_log!("Provided URL matches the Slug");
                            true
                        } else {
                            return Response::error(format!("Unable to register the hash"), 500);
                        }
                    }
                    None => {
                        console_log!("Found a new Slug for the given URL");
                        false
                    }
                };
            }
            (slug, is_slug_in_db)
        }
        None => {
            let (slug, is_slug_in_db) = generate_slug(url.clone(), &d1).await;
            (slug, is_slug_in_db)
        }
    };

    let shorten = ShortenUrl {
        url,
        slug: slug.clone(),
    };
    if !is_slug_in_db {
        match add_to_db(shorten.clone(), &d1).await {
            Ok(_) => console_log!("Slug {} inserted to the DB", slug),
            Err(_) => return Response::error(format!("Unable to register the hash"), 500),
        }
    }
    return Response::from_json(&json!(shorten));
}

fn extract_db(ctx: &RouteContext<()>) -> D1Database {
    return ctx.env.d1("DB").expect("Unable to access the D1 database");
}

async fn handle_get_url(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let slug = match ctx.param("slug") {
        Some(slug) => slug,
        None => return Response::error("Expected hash", 400),
    };
    let d1 = extract_db(&ctx);
    match retrieve_slug_from_db(slug.clone(), &d1).await {
        Some(item) => {
            console_log!("Hash found!");
            let url = Url::parse(item.url.as_str())?;
            return Response::redirect(url);
        }
        None => {
            console_log!("Send NOT FOUND error");
            return Response::error(format!("URL for hash {} not found", slug), 400);
        }
    }
}

async fn handle_home(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let template = HomeTemplate {};
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
#[template(path = "home.html")]
struct HomeTemplate;
