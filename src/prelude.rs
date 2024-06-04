pub use crate::errors::UploaderError;
use std::fmt::Debug;
use std::path::Path;
pub(crate) use std::result::Result as StdResult;

pub type Result<T> = StdResult<T, UploaderError>;

pub(crate) use tracing::{debug, error, info, trace, warn};
pub(crate) use twba_common::prelude::twba_local_db;
pub trait EasyString: Into<String> + Clone + Debug + Send + Sync {}
impl<T> EasyString for T where T: Into<String> + Clone + Debug + Send + Sync {}

pub trait EasyPath: AsRef<Path> + Debug + Send + Sync {}
impl<T> EasyPath for T where T: AsRef<Path> + Debug + Send + Sync {}
