use crate::prelude::*;
use crate::CONF;
use anyhow::{anyhow, Context};
use google_youtube3::api::Scope;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tracing::instrument;
use tracing_subscriber::fmt::format;
use twba_local_db::entities::video_upload::{ActiveModel as VideoUploadActiveModel, UploadStatus};
use twba_local_db::prelude::*;
use twba_local_db::re_exports::sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel,
    Order, QueryFilter, QueryOrder,
};

mod youtube;

lazy_static! {
    static ref YOUTUBE_DEFAULT_SCOPES: Vec<Scope> =
        vec![Scope::Upload, Scope::Readonly, Scope::Full];
}
#[derive(Debug)]
pub struct UploaderClient {
    db: DatabaseConnection,
    reqwest_client: reqwest::Client,
    youtube_client: HashMap<String, youtube::YoutubeClient>,
}

impl UploaderClient {
    #[tracing::instrument(skip(self))]
    pub(crate) async fn upload_videos(&self) -> Result<()> {
        let videos = Videos::find()
            .filter(VideosColumn::Status.eq(Status::Split))
            .order_by(VideosColumn::CreatedAt, Order::Asc)
            .all(&self.db)
            .await?;
        let count = videos.len();
        info!("got {} videos to upload", count);

        'video_loop: for video in videos {
            match self.upload_video(&video).await {
                Ok(_) => {
                    info!("Uploaded video: {}: {}", video.id, video.name);
                }
                Err(e) => {
                    error!("Error while uploading the video: {}: {}", video.id, e);

                    {
                        let fail_count = video.fail_count + 1;
                        let previous_fails = video
                            .fail_reason
                            .as_ref()
                            .unwrap_or(&String::new())
                            .to_string();
                        let mut video = video.clone().into_active_model();
                        video.fail_count = ActiveValue::Set(fail_count);
                        video.fail_reason = ActiveValue::Set(Some(format!(
                            "{}: {}\n\n{}",
                            fail_count, e, previous_fails
                        )));
                    }
                    // self.set_video_status_on_db(&video, Status::UploadFailed)
                    //     .await?;
                }
            }
        }

        //todo: maybe add some log to the db when videos were last uploaded?
        Ok(())
    }

    #[instrument(skip(self, video), fields(id=video.id))]
    async fn upload_video(&self, video: &VideosModel) -> Result<()> {
        let video_id = video.id;
        trace!("uploading video: {:?}", video);
        let client_for_video = self.get_client_for_video(video)?;

        self.set_video_status_on_db(video, Status::Uploading)
            .await?;

        let part_count = video.part_count;
        let parts_folder_path = Path::new(&CONF.download_folder_path).join(video_id.to_string());
        let parts = get_part_files(&parts_folder_path, part_count).await?;
        dbg!(&parts);

        let playlist_id = client_for_video.create_playlist(video).await?;
        self.set_playlist_id_for_video(video, playlist_id.clone())
            .await?;

        'part_loop: for (part, part_number) in parts {
            let mut video_upload = self
                .insert_video_upload(video_id, part_number)
                .await?
                .into_active_model();

            let upload = client_for_video
                .upload_video_part(video, &part, part_number)
                .await;
            match upload {
                Ok(uploaded_video_id) => {
                    dbg!(&uploaded_video_id);
                    client_for_video
                        .add_video_to_playlist(uploaded_video_id.clone(), playlist_id.clone())
                        .await?;
                    video_upload.upload_status = ActiveValue::Set(UploadStatus::Uploaded);
                    video_upload.youtube_video_id = ActiveValue::Set(Some(uploaded_video_id));
                    video_upload = video_upload.update(&self.db).await?.into_active_model();
                }
                Err(e) => {
                    error!("could not upload part: {}", e);
                    return Err(e);
                }
            }

            self.set_video_status_on_db(video, Status::PartiallyUploaded)
                .await?;
        }

        self.set_video_status_on_db(video, Status::Uploaded).await?;
        Ok(())
    }

    async fn insert_video_upload(
        &self,
        video_id: i32,
        part_number: usize,
    ) -> Result<VideoUploadModel> {
        let video_upload = VideoUploadModel {
            video_id,
            part: part_number as i32,
            upload_status: UploadStatus::Uploading,
            youtube_video_id: None,
        }
        .into_active_model();
        let x = VideoUpload::insert(video_upload);
        let x = x.exec_with_returning(&self.db).await?;
        Ok(x)
    }

    async fn set_playlist_id_for_video(
        &self,
        video: &VideosModel,
        playlist_id: String,
    ) -> Result<()> {
        let mut video = video.clone().into_active_model();
        video.youtube_playlist_id = ActiveValue::Set(Some(playlist_id));
        video.update(&self.db).await?;
        Ok(())
    }
    async fn add_video_to_playlist(&self, video: &VideosModel, playlist_id: String) -> Result<()> {
        let mut video = video.clone().into_active_model();
        video.youtube_playlist_id = ActiveValue::Set(Some(playlist_id));
        video.update(&self.db).await?;
        Ok(())
    }

    #[tracing::instrument(skip(self, video))]
    async fn set_video_status_on_db(&self, video: &VideosModel, status: Status) -> Result<()> {
        trace!("setting status of video {} to {:?}", video.id, status);
        let mut active_video = video.clone().into_active_model();
        active_video.status = ActiveValue::Set(status);
        active_video
            .update(&self.db)
            .await
            .context("could not save video status")?;
        Ok(())
    }
    #[tracing::instrument(skip(self, video_upload))]
    async fn set_video_upload_status_on_db(
        &self,
        video_upload: &VideoUploadModel,
        status: UploadStatus,
    ) -> Result<()> {
        trace!(
            "setting status of video upload {}:{} to {:?}",
            video_upload.video_id,
            video_upload.part,
            status
        );
        let mut active_video = video_upload.clone().into_active_model();
        active_video.upload_status = ActiveValue::Set(status);
        active_video
            .update(&self.db)
            .await
            .context("could not save video upload status")?;
        Ok(())
    }
    fn get_client_for_video(&self, video: &VideosModel) -> Result<&youtube::YoutubeClient> {
        let c = self
            .youtube_client
            .get(&video.id.to_string())
            .context("could not get youtube client for video")?;
        Ok(c)
    }
}

async fn get_part_files(folder_path: &Path, count: i32) -> Result<Vec<(PathBuf, usize)>> {
    let mut parts = Vec::new();
    trace!(
        "getting {} parts from folder '{}'",
        count,
        folder_path.display()
    );
    let x = folder_path
        .read_dir()
        .context("could not read parts folder")?;
    for path in x {
        let path = path.context("could not read path")?;
        let path = path.path();
        let part_number = get_part_number_from_path(&path)?;
        dbg!(part_number);
        parts.push((path, part_number));
    }
    if parts.len() != count as usize {
        return Err(anyhow!(
            "part count does not match: expected: {}, got: {}",
            count,
            parts.len()
        )
        .into());
    }
    parts.sort_by_key(|a| a.1);
    Ok(parts)
}

fn get_part_number_from_path(path: &Path) -> Result<usize> {
    match path.extension() {
        None => {
            warn!("path has no extension: {:?}", path);
        }
        Some(e) => {
            if e == OsStr::new("mp4") {
                let part_number = path
                    .file_stem()
                    .context("could not get file stem")?
                    .to_str()
                    .context("could not convert path to string")?
                    .to_string();
                let part_number = part_number
                    .parse::<usize>()
                    .context("could not parse path")?;
                return Ok(part_number);
            }
            warn!("path has not the expected extension (.mp4): {:?}", path);
        }
    }
    Err(anyhow!("wrong file extension").into())
}

impl UploaderClient {
    pub async fn new(db: DatabaseConnection) -> Result<Self> {
        let reqwest_client = reqwest::Client::new();

        let mut clients = HashMap::new();

        let users = twba_local_db::get_watched_users(&db).await?;
        for user in users {
            let user_id = user.id.to_string();
            let client = youtube::YoutubeClient::new(&YOUTUBE_DEFAULT_SCOPES, Some(user)).await?;
            clients.insert(user_id, client);
        }
        if clients.is_empty() {
            //insert default user/client
            let client = youtube::YoutubeClient::new(&YOUTUBE_DEFAULT_SCOPES, None).await?;
            clients.insert("unknown".into(), client);
        }

        Ok(Self {
            db,
            reqwest_client,
            youtube_client: clients,
        })
    }
}
