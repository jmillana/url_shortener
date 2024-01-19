use md5;
use warp::Filter;
pub fn shorten(url: String) -> String {
    let digest = md5::compute(url);
    return format!("{:x}", digest);
}

#[tokio::main]
async fn main() {
    let hello = warp::path!("hello" / String).map(|name| format!("Hello, {}!", name));
    let routes = shorten_url.or(hello);
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await
