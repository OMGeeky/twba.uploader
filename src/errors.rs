#[derive(Debug, thiserror::Error)]
pub enum UploaderError {
    #[error("Could not load config")]
    LoadConfig(#[source] anyhow::Error),

    #[error("Some error with the database")]
    OpenDatabase(#[from] local_db::re_exports::sea_orm::DbErr),

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
