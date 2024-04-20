#![allow(unused)]
use twba_backup_config::prelude::*;
use lazy_static::lazy_static;

use prelude::*;

mod client;
pub mod errors;
pub mod prelude;

lazy_static! {
    pub(crate) static ref CONF: Conf = Conf::builder()
        .env()
        .file(
            std::env::var("TWBA_CONFIG")
                .map(|v| {
                    dbg!(&v);
                    info!("using {} as primary config source after env", v);
                    v
                })
                .unwrap_or_else(|x| {
                    dbg!(x);
                    error!("could not get config location from env");
                    "./settings.toml".to_string()
                })
        )
        .file("./settings.toml")
        .file(shellexpand::tilde("~/twba/config.toml").into_owned())
        .load()
        .map_err(|e| UploaderError::LoadConfig(e.into()))
        .expect("Failed to load config");
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_env_filter("warn,uploader=trace")
        .init();
    let args = std::env::args().collect::<Vec<_>>();
    let presentation_mode = args.len() > 1;
    info!("Hello, world!");

    run().await?;

    info!("Bye");
    Ok(())
}

#[tracing::instrument]
async fn run() -> Result<()> {
    trace!("run");
    let x = &CONF.google;
    debug!("{:?}", x);

    trace!("creating db-connection with db url: {}", &CONF.db_url);
    let db = twba_local_db::open_database(Some(&CONF.db_url)).await?;
    trace!("migrating db");
    twba_local_db::migrate_db(&db).await?;
    // local_db::print_db(&db).await?;

    trace!("creating client");
    // dbg!(&conf);

    let client = client::UploaderClient::new(db).await?;
    trace!("uploading videos");
    client.upload_videos().await?;

    Ok(())
}
