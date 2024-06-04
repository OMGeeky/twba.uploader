use lazy_static::lazy_static;
use twba_common::prelude::*;

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
    info!("Hello, world!");

    run().await?;

    info!("Bye");
    Ok(())
}

#[tracing::instrument]
async fn run() -> Result<()> {
    trace!("run");

    trace!("creating db-connection with db url: {}", &CONF.db_url);
    let db = twba_local_db::open_database(Some(&CONF.db_url)).await?;
    trace!("migrating db");
    twba_local_db::migrate_db(&db).await?;

    trace!("creating client");
    let client = client::UploaderClient::new(db).await?;
    trace!("uploading videos");
    client.upload_videos().await?;

    Ok(())
}
