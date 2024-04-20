#[derive(Debug, thiserror::Error)]
pub enum UploaderError {
    #[error("Path could not be expanded")]
    ExpandPath(#[source] anyhow::Error),

    #[error("Could not load config")]
    LoadConfig(#[source] anyhow::Error),

    #[error("Some error with the database: {0:?}")]
    OpenDatabase(#[from] twba_local_db::re_exports::sea_orm::DbErr),

    #[error("Error with some Youtube operation: {0} ")]
    YoutubeError(#[source] google_youtube3::Error),

    #[error("Temporary error. Remove for production, {0}")]
    //TODO: Remove this error
    Tmp1(#[from] anyhow::Error),
    #[error("Temporary error. Remove for production, {0}")]
    //TODO: Remove this error
    Tmp3(#[from] google_youtube3::Error),
    #[error("Temporary error. Remove for production")]
    //TODO: Remove this error
    Tmp2,
}
