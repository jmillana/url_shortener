use md5;
use warp::Filter;
extern crate dotenv;
extern crate mysql;
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

fn with_db(
    db_pool: mysql::Pool,
) -> impl Filter<Extract = (mysql::Pool,), Error = Infallible> + Clone {
    warp::any().map(move || db_pool.clone())
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
    let routes = shorten_url.or(hello);
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await
}
