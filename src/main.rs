#![allow(unused)]
use lazy_static::lazy_static;
use twba_backup_config::prelude::*;
use twba_common::{get_config, init_tracing};

use prelude::*;

mod client;
pub mod errors;
pub mod prelude;

lazy_static! {
    pub(crate) static ref CONF: Conf = get_config();
}
#[tokio::main]
async fn main() -> Result<()> {
    let _guard = init_tracing("twba_uploader");
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
    let conf = &CONF.clone();
    dbg!(conf);

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
