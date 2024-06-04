use crate::client::youtube::data::VideoData;
use crate::client::youtube::data::{create_youtube_description, create_youtube_title};
use crate::prelude::*;
use crate::CONF;
use google_youtube3::api::enums::{PlaylistStatusPrivacyStatusEnum, VideoStatusPrivacyStatusEnum};
use google_youtube3::api::Scope;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tracing::instrument;
use twba_local_db::entities::video_upload::UploadStatus;
use twba_local_db::prelude::*;
use twba_local_db::re_exports::sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel,
    Order, QueryFilter, QueryOrder, QuerySelect,
};
use youtube::data::Location;

mod youtube;

lazy_static! {
    static ref YOUTUBE_DEFAULT_SCOPES: Vec<Scope> =
        vec![Scope::Upload, Scope::Readonly, Scope::Full];
}
#[derive(Debug)]
pub struct UploaderClient {
    db: DatabaseConnection,
    youtube_clients: HashMap<String, youtube::YoutubeClient>,
}

impl UploaderClient {
    #[tracing::instrument(skip(self))]
    pub(crate) async fn upload_videos(&self) -> Result<()> {
        let videos = Videos::find()
            .filter(VideosColumn::Status.eq(Status::Split))
            .order_by(VideosColumn::CreatedAt, Order::Asc)
            .limit(CONF.max_items_to_process)
            .all(&self.db)
            .await?;
        let count = videos.len();
        info!("got {} videos to upload", count);

        for video in videos {
            match self.upload_video(&video).await {
                Ok(_) => {
                    info!("Uploaded video: {}: {}", video.id, video.name);
                }
                Err(e) => {
                    error!("Error while uploading the video: {}: {}", video.id, e);

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
            }
        }

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
        let user = Users::find_by_id(video.user_id)
            .one(&self.db)
            .await?
            .ok_or(UploaderError::UnknownUser(video.user_id))?;

        let tags = vec![];
        let all_parts_data = VideoData {
            video_tags: tags,
            video_category: 22,
            video_privacy: VideoStatusPrivacyStatusEnum::Private,
            playlist_privacy: PlaylistStatusPrivacyStatusEnum::Private,
            playlist_description: create_youtube_description(video, &user, Location::Playlist)?,
            playlist_title: create_youtube_title(video, &user, Location::Playlist)?,
            //The rest of the fields are filled in the loop
            part_number: 0,
            video_title: "".to_string(),
            video_description: "".to_string(),
        };
        let playlist_id = client_for_video.create_playlist(&all_parts_data).await?;
        self.set_playlist_id_for_video(video, playlist_id.clone())
            .await?;

        for (part, part_number) in parts {
            let mut video_upload = self
                .insert_video_upload(video_id, part_number)
                .await?
                .into_active_model();

            let data = VideoData {
                part_number,
                video_title: create_youtube_title(video, &user, Location::Video(part_number))?,
                video_description: create_youtube_description(
                    video,
                    &user,
                    Location::Video(part_number),
                )?,
                ..all_parts_data.clone()
            };
            trace!(
                "uploading part {} for video: {} from path: {}",
                part_number,
                video.id,
                part.display()
            );
            let upload = client_for_video.upload_video_part(&part, data).await;
            match upload {
                Ok(uploaded_video_id) => {
                    info!("uploaded part: {}", part.display());
                    dbg!(&uploaded_video_id);
                    client_for_video
                        .add_video_to_playlist(uploaded_video_id.clone(), playlist_id.clone())
                        .await?;
                    video_upload.upload_status = ActiveValue::Set(UploadStatus::Uploaded);
                    video_upload.youtube_video_id = ActiveValue::Set(Some(uploaded_video_id));
                    video_upload.update(&self.db).await?;
                }
                Err(e) => {
                    error!("could not upload part: {}", e);
                    return Err(e);
                }
            }

            self.set_video_status_on_db(video, Status::PartiallyUploaded)
                .await?;
        }

        info!("all parts uploaded for video: {}", video_id);
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

    #[tracing::instrument(skip(self, video))]
    async fn set_video_status_on_db(&self, video: &VideosModel, status: Status) -> Result<()> {
        trace!("setting status of video {} to {:?}", video.id, status);
        let mut active_video = video.clone().into_active_model();
        active_video.status = ActiveValue::Set(status);
        active_video
            .update(&self.db)
            .await
            .map_err(UploaderError::SaveVideoStatus)?;
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
            .map_err(UploaderError::SaveVideoStatus)?;
        Ok(())
    }
    fn get_client_for_video(&self, video: &VideosModel) -> Result<&youtube::YoutubeClient> {
        let c = self
            .youtube_clients
            .get(&video.user_id.to_string())
            .ok_or(UploaderError::NoClient(video.user_id))?;
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
        .map_err(UploaderError::ReadPartsFolder)?;
    for path in x {
        let path = path.map_err(UploaderError::OpenPartFile)?;
        let path = path.path();
        let part_number = get_part_number_from_path(&path)?;
        dbg!(part_number);
        parts.push((path, part_number));
    }
    if parts.len() != count as usize {
        return Err(UploaderError::PartCountMismatch(
            count as usize,
            parts.len(),
        ));
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
                    .ok_or(UploaderError::GetNameWithoutFileExtension)?
                    .to_str()
                    .ok_or(UploaderError::ConvertPathToString)?
                    .to_string();
                let part_number = part_number
                    .parse::<usize>()
                    .map_err(UploaderError::ParsePartNumber)?;
                return Ok(part_number + 1);
            }
            warn!("path has not the expected extension (.mp4): {:?}", path);
        }
    }
    Err(UploaderError::WrongFileExtension)
}

impl UploaderClient {
    pub async fn new(db: DatabaseConnection) -> Result<Self> {
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
            youtube_clients: clients,
        })
    }
}
