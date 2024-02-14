use anyhow::Result;
use axum::Router;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "./data")]
    data_path: String,

    #[arg(short, long, default_value = "local_dev")]
    secret: String,

    #[arg(short, long, default_value = "/")]
    base_url: String,

    #[arg(short, long, default_value = "3231")]
    port: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt::init();

    std::fs::create_dir_all(&args.data_path)?;

    let db_path = std::path::Path::new(&args.data_path)
        .join("db.sqlite")
        .to_string_lossy()
        .to_string();

    let db_url = format!("sqlite://{}", db_path);

    let db = database::Database::new(&db_url).await?;
    let frontend = frontend::initialize(&args.base_url, &args.secret, args.data_path, db).await?;

    let app = Router::new().nest(&args.base_url, frontend);

    let addr = format!("0.0.0.0:{}", args.port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
