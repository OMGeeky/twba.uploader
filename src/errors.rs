use shellexpand::LookupError;
use std::env::VarError;
use std::path::PathBuf;
use strfmt::FmtError;

#[derive(Debug, thiserror::Error)]
pub enum UploaderError {
    #[error("Path could not be expanded")]
    ExpandPath(#[source] LookupError<VarError>),

    #[error("Got an auth error: {0}")]
    AuthError(#[from] AuthError),

    #[error("Some error with the database: {0:?}")]
    Database(#[from] twba_local_db::re_exports::sea_orm::DbErr),

    #[error("Error with some Youtube operation: {0} ")]
    YoutubeError(#[source] google_youtube3::Error),

    #[error("Could not find user: {0}")]
    UnknownUser(i32),
    #[error("Could not find client for user: {0}")]
    NoClient(i32),
    #[error("Could not read part file: {0}")]
    OpenPartFile(#[source] std::io::Error),
    #[error("Could not read parts folder: {0}")]
    ReadPartsFolder(#[source] std::io::Error),
    #[error("Could not delete part file after uploading: {0}")]
    DeletePartAfterUpload(#[source] std::io::Error),
    #[error("wrong file extension")]
    WrongFileExtension,
    #[error("could not get file stem")]
    GetNameWithoutFileExtension,
    #[error("could not convert path to string")]
    ConvertPathToString,
    #[error("could not parse part number from path: {0}")]
    ParsePartNumber(#[source] std::num::ParseIntError),
    #[error("could not save video status")]
    SaveVideoStatus(#[source] twba_local_db::re_exports::sea_orm::DbErr),
    #[error("could not parse date: {0}")]
    ParseDate(#[source] chrono::ParseError),
    #[error("part count does not match: expected: {0}, got: {1}")]
    PartCountMismatch(usize, usize),
    #[error("no id returned from youtube")]
    NoIdReturned,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("could not read application secret from path: {0}")]
    ReadApplicationSecret(#[source] std::io::Error),
    #[error("could not create auth")]
    CreateAuth(#[source] std::io::Error),
    #[error("could not get access to the requested scopes")]
    GetAccessToken(#[source] Box<dyn std::error::Error>),
    #[error("could not get and validate persistent path: {0}")]
    PersistentPathError(#[from] PersistentPathError),
    #[error("could not remove existing auth code file: {0}")]
    RemoveAuthCodeFile(#[source] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum PersistentPathError {
    #[error("persistent path parent folder is not a dir: {0}")]
    PathNotDir(PathBuf),
    #[error("could not replace user in persistent path")]
    ReplaceUser(#[source] FmtError),
    #[error("could not get parent folder")]
    GetParentFolder,
    #[error("could not create dirs")]
    CreateDirs(#[source] std::io::Error),
}
