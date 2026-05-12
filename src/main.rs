#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("import") {
        run_import(&args[1..])?;
    }
    Ok(())
}

fn run_import(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let file = args
        .first()
        .ok_or("usage: feralctf import <file> [--attachments <dir>] [--overwrite] [--dry-run]")?;
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

    let config = feralctf::config::load("config.toml")?;
    let pool = feralctf::db::init_pool(&config.database.path)?;
    let conn = pool.get()?;
    let raw = std::fs::read(file)?;
    let bundle = feralctf::import_export::detect_and_convert_ctfd(&raw)?;
    let options = feralctf::import_export::ImportOptions { overwrite, dry_run };
    let result = feralctf::import_export::import(&conn, &bundle, attachments.as_deref(), &options)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
