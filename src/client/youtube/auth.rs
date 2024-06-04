use crate::client::youtube::flow_delegate::CustomFlowDelegate;
use crate::errors::{AuthError, PersistentPathError};
use crate::prelude::*;
use google_youtube3::api::Scope;
use google_youtube3::oauth2::authenticator::Authenticator;
use google_youtube3::{hyper::client::HttpConnector, hyper_rustls::HttpsConnector, oauth2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::instrument;

type Result<T> = std::result::Result<T, AuthError>;
#[instrument]
pub(super) async fn get_auth<USER: EasyString>(
    application_secret_path: &impl EasyPath,
    scopes: &Vec<Scope>,
    user: Option<USER>,
) -> Result<Authenticator<HttpsConnector<HttpConnector>>> {
    let application_secret_path = application_secret_path.as_ref();
    trace!(
        "getting auth for user: {:?} with scopes: {:?} and secret_path: {:?}",
        user,
        scopes,
        application_secret_path
    );

    let app_secret = oauth2::read_application_secret(application_secret_path)
        .await
        .map_err(AuthError::ReadApplicationSecret)?;

    let persistent_path =
        get_and_validate_persistent_path(&crate::CONF.google.path_auth_cache, user.clone()).await?;
    trace!(
        "persistent path for auth for user: {:?}: {:?}",
        user,
        &persistent_path
    );

    trace!("creating authenticator");
    let user = user.map(|x| x.into());
    let method = oauth2::InstalledFlowReturnMethod::Interactive;
    let auth = oauth2::InstalledFlowAuthenticator::builder(app_secret, method)
        .flow_delegate(Box::new(CustomFlowDelegate::new(user)))
        .persist_tokens_to_disk(persistent_path)
        .force_account_selection(true)
        .build()
        .await
        .map_err(AuthError::CreateAuth)?;

    trace!("got authenticator, requesting scopes");
    let access_token = auth
        .token(scopes)
        .await
        .map_err(|e| AuthError::GetAccessToken(e.into()))?;
    trace!("got scope access: {:?}", access_token);
    Ok(auth)
}

async fn get_and_validate_persistent_path<TEMPLATE: EasyString, USER: EasyString>(
    persistent_path_template: TEMPLATE,
    user: Option<USER>,
) -> Result<PathBuf> {
    let persistent_path = get_persistent_path(persistent_path_template, user.clone())?;
    let persistent_path = Path::new(&persistent_path);
    info!(
        "Persistent auth path for user:{:?} => {}",
        user,
        persistent_path.display()
    );

    if persistent_path.is_dir() {
        warn!("persistent path is a dir: {}", persistent_path.display());
    }

    let persistent_path_parent_folder = persistent_path
        .parent()
        .ok_or(PersistentPathError::GetParentFolder)?;
    if !persistent_path_parent_folder.exists() {
        debug!(
            "persistent path parent folder does not exist, creating it: {}",
            persistent_path_parent_folder.display()
        );
        fs::create_dir_all(persistent_path_parent_folder)
            .await
            .map_err(PersistentPathError::CreateDirs)?;
    } else if !persistent_path_parent_folder.is_dir() {
        error!(
            "persistent path parent folder is not a dir: {}",
            persistent_path_parent_folder.display()
        );
        return Err(
            PersistentPathError::PathNotDir(persistent_path_parent_folder.to_path_buf()).into(),
        );
    }
    Ok(persistent_path.to_path_buf())
}

fn get_persistent_path<TEMPLATE: EasyString, USER: EasyString>(
    persistent_path_template: TEMPLATE,
    user: Option<USER>,
) -> Result<String> {
    let user: String = match user {
        Some(user) => user.into(),
        None => "unknown".to_string(),
    };
    let vars: HashMap<String, String> = HashMap::from([("user".to_string(), user)]);
    let persistent_path = strfmt::strfmt(&persistent_path_template.into(), &vars)
        .map_err(PersistentPathError::ReplaceUser)?;
    Ok(persistent_path)
}
