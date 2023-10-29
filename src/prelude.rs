pub use crate::errors::UploaderError;
use std::fmt::Debug;
pub(crate) use std::result::Result as StdResult;

pub type Result<T> = StdResult<T, UploaderError>;

pub(crate) use tracing::{debug, error, info, trace, warn};

pub trait EasyString: Into<String> + Clone + Debug + Send + Sync {}

impl<T> EasyString for T where T: Into<String> + Clone + Debug + Send + Sync {}
