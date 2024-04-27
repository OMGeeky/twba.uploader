use crate::errors::AuthError;
use crate::prelude::*;
use std::{
    fmt::{Debug, Formatter},
    future::Future,
    path::Path,
    pin::Pin,
};
use tracing::instrument;
use twba_backup_config::Conf;
use yup_oauth2::authenticator_delegate::InstalledFlowDelegate;

pub struct CustomFlowDelegate<USER: EasyString> {
    user: Option<USER>,
}

impl<USER: EasyString> Debug for CustomFlowDelegate<USER> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomFlowDelegate")
            .field("user", &self.user)
            .finish()
    }
}
impl<USER: EasyString> CustomFlowDelegate<USER> {
    pub(crate) fn new(user: Option<USER>, config: &'static Conf) -> Self {
        Self { user }
    }
}
impl<USER: EasyString> InstalledFlowDelegate for CustomFlowDelegate<USER> {
    #[tracing::instrument(skip(self))]
    fn redirect_uri(&self) -> Option<&str> {
        if !(&crate::CONF.google.local_auth_redirect) {
            let url = "https://game-omgeeky.de:7443/googleapi/auth";
            trace!("server redirect uri: {}", url);
            Some(url)
        } else {
            let url = "http://localhost:8080/googleapi/auth";
            trace!("local redirect uri: {}", url);
            Some(url)
        }
    }
    fn present_user_url<'a>(
        &'a self,
        url: &'a str,
        need_code: bool,
    ) -> Pin<Box<dyn Future<Output = StdResult<String, String>> + Send + 'a>> {
        Box::pin(self.present_user_url(url, need_code))
    }
}
impl<USER: EasyString> CustomFlowDelegate<USER> {
    #[tracing::instrument(skip(self, url, need_code))]
    async fn present_user_url(&self, url: &str, need_code: bool) -> StdResult<String, String> {
        let user: String = self
            .user
            .clone()
            .map(|x| x.into())
            .unwrap_or_else(|| "unknown".into());
        let message = format!(
            "Please open this URL in your browser to authenticate for {}:\n{}\n",
            user, url
        );
        println!("{}", message);
        info!("{}", message);
        if need_code {
            let mut code = String::new();
            if crate::CONF.google.use_file_auth_response {
                code = get_auth_code().await.unwrap_or("".to_string());
            }
            if code.is_empty() {
                println!("Please enter the code provided: ");
                match std::io::stdin().read_line(&mut code) {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error reading line: {}", e);
                        return Err("".into());
                    }
                }
            }
            Ok(code)
        } else {
            Ok("".to_string())
        }
    }
}
#[instrument]
async fn get_auth_code() -> Result<String> {
    let code: String;

    let path = Path::new(&crate::CONF.google.path_auth_code);
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            println!("Error removing file: {}", e);
            error!("Error removing file: {}", e);
            return Err(AuthError::RemoveAuthCodeFile(e).into());
        }
    }
    let message = format!("Waiting for auth code in file: {}", path.display());
    println!("{}", message);
    info!(message);
    loop {
        let res = std::fs::read_to_string(path);
        if let Ok(content) = res {
            let line = content.lines().next();
            let line = match line {
                Some(s) => s.to_string(),
                None => {
                    let message = "No code found in file";
                    println!("{}", message);
                    info!(message);
                    continue;
                }
            };
            code = line;
            break;
        }

        println!(
            "sleeping for {} second before trying again",
            crate::CONF.google.auth_file_read_timeout
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(
            crate::CONF.google.auth_file_read_timeout,
        ))
        .await;
    }

    Ok(code)
}
