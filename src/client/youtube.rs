use crate::client::data::VideoData;
use crate::prelude::{info, trace, Result, UploaderError};
use google_youtube3::{
    api::{
        Playlist, PlaylistItem, PlaylistItemSnippet, PlaylistSnippet, PlaylistStatus, ResourceId,
        Scope, Video, VideoSnippet, VideoStatus,
    },
    hyper::{self, client::HttpConnector, Client},
    hyper_rustls::{HttpsConnector, HttpsConnectorBuilder},
};
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::instrument;

mod auth;
mod flow_delegate;

pub struct YoutubeClient {
    //TODO: change this to a thing that does exponential backoff when possible
    client: google_youtube3::YouTube<HttpsConnector<HttpConnector>>,
}

impl YoutubeClient {
    #[instrument(skip(self, path, data))]
    pub(crate) async fn upload_video_part(&self, path: &Path, data: VideoData) -> Result<String> {
        let video_data = data;
        let upload_result = self
            .upload_youtube_video_resumable(video_data, path)
            .await?;
        fs::remove_file(path)
            .await
            .map_err(UploaderError::DeletePartAfterUpload)?;
        Ok(upload_result)
    }

    async fn upload_youtube_video_resumable(
        &self,
        video_data: VideoData,
        path: &Path,
    ) -> Result<String> {
        let video = Video {
            snippet: Some(VideoSnippet {
                title: Some(video_data.video_title),
                description: Some(video_data.video_description),
                category_id: Some(video_data.video_category.to_string()),
                tags: Some(video_data.video_tags),
                ..Default::default()
            }),
            status: Some(VideoStatus {
                privacy_status: Some(video_data.video_privacy),
                public_stats_viewable: Some(true),
                embeddable: Some(true),
                self_declared_made_for_kids: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        let stream = fs::File::open(path)
            .await
            .map_err(UploaderError::OpenPartFile)?;

        let insert_call = self.client.videos().insert(video);
        trace!("Starting resumable upload");
        let upload = insert_call
            .upload_resumable(
                stream.into_std().await,
                "video/mp4".parse().map_err(|_| {
                    UploaderError::Unreachable(
                        "Could not parse 'video/mp4' mime type. This mime type needs to always be valid.".to_string(),
                    )
                })?,
            )
            .await;
        trace!("Resumable upload finished");
        let result_str = if upload.is_ok() { "Ok" } else { "Error" };
        info!("upload request done with result: {}", result_str);
        upload
            .map_err(UploaderError::YoutubeError)?
            .1
            .id
            .ok_or(UploaderError::NoIdReturned)
    }
}

impl YoutubeClient {
    #[instrument(skip(self))]
    pub(crate) async fn add_video_to_playlist(
        &self,
        uploaded_video_id: String,
        playlist_id: String,
    ) -> Result<()> {
        let playlist_item = PlaylistItem {
            snippet: Some(PlaylistItemSnippet {
                playlist_id: Some(playlist_id),
                resource_id: Some(ResourceId {
                    kind: Some("youtube#video".to_string()),
                    video_id: Some(uploaded_video_id),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        self.client
            .playlist_items()
            .insert(playlist_item)
            .doit()
            .await
            .map_err(UploaderError::YoutubeError)?;
        Ok(())
    }
    #[instrument(skip(self, video))]
    pub(crate) async fn create_playlist(&self, video: &VideoData) -> Result<String> {
        trace!("creating playlist for video: {:?}", video);
        trace!("title: {}", video.playlist_title);
        trace!("description: {:?}", video.playlist_description);
        trace!("privacy: {:?}", video.playlist_privacy);

        let playlist = Playlist {
            snippet: Some(PlaylistSnippet {
                title: Some(video.playlist_title.clone()),
                description: Some(video.playlist_description.clone()),
                ..Default::default()
            }),
            status: Some(PlaylistStatus {
                privacy_status: Some(video.playlist_privacy),
            }),
            ..Default::default()
        };
        let playlist_insert_call = self.client.playlists().insert(playlist);
        let (_, playlist) = playlist_insert_call
            .doit()
            .await
            .map_err(UploaderError::YoutubeError)?;

        playlist.id.ok_or(UploaderError::NoIdReturned)
    }
}

impl Debug for YoutubeClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YoutubeClient").finish()
    }
}

impl YoutubeClient {
    #[tracing::instrument]
    pub async fn new(scopes: &Vec<Scope>, user: Option<String>) -> Result<Self> {
        let hyper_client = Self::create_hyper_client()?;
        let application_secret_path = PathBuf::from(
            &shellexpand::full(&crate::CONF.google.youtube.client_secret_path)
                .map_err(UploaderError::ExpandPath)?
                .to_string(),
        );

        let auth = auth::get_auth(&application_secret_path, scopes, user).await?;
        let client = google_youtube3::YouTube::new(hyper_client, auth);
        Ok(Self { client })
    }

    fn create_hyper_client() -> Result<Client<HttpsConnector<HttpConnector>>> {
        Ok(hyper::Client::builder().build(
            HttpsConnectorBuilder::new()
                .with_native_roots()
                .map_err(UploaderError::CreateClient)?
                .https_or_http()
                .enable_http1()
                .enable_http2()
                .build(),
        ))
    }
}
