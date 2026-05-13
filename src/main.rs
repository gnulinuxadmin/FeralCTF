use std::{net::SocketAddr, path::Path, sync::Arc};

#[derive(Debug)]
struct Cli {
    config_path: String,
    port: Option<u16>,
    command: Command,
}

#[derive(Debug)]
enum Command {
    Serve,
    Init,
    Migrate,
    Import {
        file: String,
        attachments: Option<std::path::PathBuf>,
        overwrite: bool,
        dry_run: bool,
    },
}

const HELP: &str = "\
feralctf - self-hosted CTF platform

USAGE:
    feralctf [OPTIONS] [COMMAND]

OPTIONS:
    --port <PORT>        Listen on PORT (overrides config)
    --config <PATH>      Load config from PATH (default: config.toml)
    --version            Print version and exit
    --help               Print this help and exit

COMMANDS:
    (none)               Start the server
    init                 Create config.toml, ctf.db, and attachments/
    migrate              Apply schema migrations to existing database
    import <FILE>        Import challenge bundle
        --dry-run        Preview without writing
        --overwrite      Replace challenges with matching slug
        --attachments    Directory of attachment files to copy
";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = parse_args(std::env::args().skip(1).collect())?;
    match cli.command {
        Command::Serve => run_server(&cli.config_path, cli.port).await?,
        Command::Init => run_init(&cli.config_path)?,
        Command::Migrate => run_migrate(&cli.config_path)?,
        Command::Import {
            file,
            attachments,
            overwrite,
            dry_run,
        } => run_import(
            &cli.config_path,
            &file,
            attachments.as_deref(),
            overwrite,
            dry_run,
        )?,
    }
    Ok(())
}

fn parse_args(mut args: Vec<String>) -> Result<Cli, Box<dyn std::error::Error>> {
    // FERALCTF_SPEC.md §2.1 / §9.2 define the single-binary command surface.
    // Keep this parser dependency-free so release builds remain self-contained.
    let mut config_path = "config.toml".to_string();
    let mut port = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("feralctf {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--config" => {
                config_path = args
                    .get(index + 1)
                    .ok_or("--config requires a path")?
                    .clone();
                args.drain(index..=index + 1);
            }
            "--port" => {
                let value = args.get(index + 1).ok_or("--port requires a value")?;
                port = Some(value.parse()?);
                args.drain(index..=index + 1);
            }
            _ => index += 1,
        }
    }

    let command = match args.first().map(String::as_str) {
        None => Command::Serve,
        Some("init") => Command::Init,
        Some("migrate") => Command::Migrate,
        Some("import") => parse_import(&args[1..])?,
        Some(other) => return Err(format!("unknown command: {other}").into()),
    };

    Ok(Cli {
        config_path,
        port,
        command,
    })
}

fn parse_import(args: &[String]) -> Result<Command, Box<dyn std::error::Error>> {
    let file = args
        .first()
        .ok_or("usage: feralctf import <file> [--attachments <dir>] [--overwrite] [--dry-run]")?
        .clone();
    let mut attachments = None;
    let mut overwrite = false;
    let mut dry_run = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--attachments" => {
                let dir = args
                    .get(index + 1)
                    .ok_or("--attachments requires a directory")?;
                attachments = Some(std::path::PathBuf::from(dir));
                index += 2;
            }
            "--overwrite" => {
                overwrite = true;
                index += 1;
            }
            "--dry-run" => {
                dry_run = true;
                index += 1;
            }
            other => return Err(format!("unknown import option: {other}").into()),
        }
    }

    Ok(Command::Import {
        file,
        attachments,
        overwrite,
        dry_run,
    })
}

fn run_init(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // FERALCTF_SPEC.md §8.1: init bootstraps config, SQLite DB, and attachments/.
    let config = default_config();
    std::fs::write(config_path, toml::to_string_pretty(&config)?)?;
    println!("Created: {config_path}");

    let pool = feralctf::db::init_pool(&config.database.path)?;
    {
        let conn = pool.get()?;
        feralctf::db::run_migrations(&conn)?;
    }
    println!("Created: {}", config.database.path);

    std::fs::create_dir_all(&config.storage.attachments_path)?;
    println!("Created: {}", config.storage.attachments_path);
    Ok(())
}

fn default_config() -> feralctf::Config {
    let mut config = feralctf::Config::default();
    config.auth.jwt_secret = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    config
}

fn run_migrate(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = feralctf::config::load(config_path)?;
    let pool = feralctf::db::init_pool(&config.database.path)?;
    let conn = pool.get()?;
    feralctf::db::run_migrations(&conn)?;
    println!("Migrations applied to {}", config.database.path);
    Ok(())
}

async fn run_server(
    config_path: &str,
    port: Option<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    // FERALCTF_SPEC.md §2.1: startup loads config, runs migrations, and starts Axum.
    let mut config = feralctf::config::load(config_path)?;
    if let Some(port) = port {
        config.server.port = port;
    }
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    let pool = feralctf::db::init_pool(&config.database.path)?;
    {
        let conn = pool.get()?;
        feralctf::db::run_migrations(&conn)?;
    }

    let rate_limiter = Arc::new(feralctf::anticheat::RateLimiter::new());
    let state = feralctf::AppState {
        db: pool,
        config: Arc::new(config),
        cache: Arc::new(feralctf::AppCache::new()),
        ws_hub: Arc::new(feralctf::WsHub::new()),
        rate_limiter: Arc::clone(&rate_limiter),
    };

    let _score_snapshots = feralctf::handlers::scoreboard::spawn_score_snapshot_task(state.clone());
    let _rate_limit_gc = feralctf::anticheat::spawn_rate_limiter_gc_task(rate_limiter);

    let app = feralctf::routes::create_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "feralctf listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn run_import(
    config_path: &str,
    file: &str,
    attachments: Option<&Path>,
    overwrite: bool,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = feralctf::config::load(config_path)?;
    let pool = feralctf::db::init_pool(&config.database.path)?;
    let conn = pool.get()?;
    let raw = std::fs::read(file)?;
    let bundle = feralctf::import_export::detect_and_convert_ctfd(&raw)?;
    let options = feralctf::import_export::ImportOptions { overwrite, dry_run };
    let result = feralctf::import_export::import(&conn, &bundle, attachments, &options)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
