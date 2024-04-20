use anyhow::Context;
use chrono::{Datelike, NaiveDateTime, ParseResult, Utc};
use std::fmt::{Debug, Formatter};
use std::path::{Path, PathBuf};

use crate::prelude::*;
use google_youtube3::api::enums::{PlaylistStatuPrivacyStatusEnum, VideoStatuPrivacyStatusEnum};
use google_youtube3::api::{
    Playlist, PlaylistSnippet, PlaylistStatus, Scope, VideoSnippet, VideoStatus,
};
use google_youtube3::api::{PlaylistItem, PlaylistItemSnippet, ResourceId, Video};
use google_youtube3::{
    hyper,
    hyper::client::HttpConnector,
    hyper::Client,
    hyper_rustls::{HttpsConnector, HttpsConnectorBuilder},
    Error as YoutubeError,
};
use tokio::fs;
use tracing::instrument;
use twba_local_db::entities::videos::Model;
use twba_local_db::prelude::{UsersModel, VideosModel};

mod auth;
mod flow_delegate;

pub struct YoutubeClient {
    //TODO: change this to a thing that does exponential backoff when possible
    client: google_youtube3::YouTube<HttpsConnector<HttpConnector>>,
    user: Option<UsersModel>,
}

impl YoutubeClient {
    #[instrument(skip(self, video, path))]
    pub(crate) async fn upload_video_part(
        &self,
        video: &VideosModel,
        path: &Path,
        part_num: usize,
    ) -> Result<String> {
        trace!(
            "uploading part {} for video: {} from path: {}",
            part_num,
            video.id,
            path.display()
        );
        let title = create_youtube_title(video, TitleLocation::VideoTitle(part_num))?;
        let description = format!(
            "default description for video: {}",
            create_youtube_title(video, TitleLocation::Descriptions)?
        );
        let tags = vec![];
        let privacy_status = VideoStatuPrivacyStatusEnum::Private;
        self.upload_youtube_video_resumable(title, description, tags, privacy_status, path)
            .await
    }

    async fn upload_youtube_video_resumable(
        &self,
        title: impl Into<String>,
        description: impl Into<String>,
        tags: impl Into<Vec<String>>,
        privacy_status: VideoStatuPrivacyStatusEnum,
        path: &Path,
    ) -> Result<String> {
        let video = Video {
            snippet: Some(VideoSnippet {
                title: Some(title.into()),
                description: Some(description.into()),
                category_id: Some("20".to_string()),
                tags: Some(tags.into()),
                ..Default::default()
            }),
            status: Some(VideoStatus {
                privacy_status: Some(privacy_status),
                public_stats_viewable: Some(true),
                embeddable: Some(true),
                self_declared_made_for_kids: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        let stream = fs::File::open(path).await.context("could not open file")?;

        let insert_call = self.client.videos().insert(video);
        trace!("Starting resumable upload");
        let upload = insert_call
            .upload_resumable(stream.into_std().await, "video/mp4".parse().unwrap())
            .await;
        trace!("Resumable upload finished");
        let result_str = if upload.is_ok() { "Ok" } else { "Error" };
        info!("upload request done with result: {}", result_str);
        upload?.1.id.ok_or(UploaderError::Tmp2)
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
            .await?;
        Ok(())
    }
    #[instrument(skip(self, video))]
    pub(crate) async fn create_playlist(&self, video: &VideosModel) -> Result<String> {
        trace!("creating playlist for video: {:?}", video);
        let title = create_youtube_title(video, TitleLocation::PlaylistTitle)?;
        trace!("title: {}", title);
        let description: Option<String> = None;
        trace!("description: {:?}", description);
        let privacy_status = PlaylistStatuPrivacyStatusEnum::Private; //TODO: Get setting per user from db
        trace!("privacy: {:?}", privacy_status);

        let playlist = Playlist {
            snippet: Some(PlaylistSnippet {
                title: Some(title),
                description,
                ..Default::default()
            }),
            status: Some(PlaylistStatus {
                privacy_status: Some(privacy_status),
            }),
            ..Default::default()
        };
        let playlist_insert_call = self.client.playlists().insert(playlist);
        let (x, playlist) = playlist_insert_call
            .doit()
            .await
            // .context("could not create playlist")
            // ?
            .unwrap()
            //test
            ;

        Ok(playlist
            .id
            .context("playlist creation did not return an ID")?)
    }
}

impl Debug for YoutubeClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YoutubeClient").finish()
    }
}

impl YoutubeClient {
    #[tracing::instrument]
    pub async fn new(scopes: &Vec<Scope>, user: Option<UsersModel>) -> Result<Self> {
        let hyper_client = Self::create_hyper_client();
        let application_secret_path = &crate::CONF.google.youtube.client_secret_path;

        let auth = auth::get_auth(
            application_secret_path,
            scopes,
            user.as_ref().map(|x| &x.youtube_id),
        )
        .await?;
        let client = google_youtube3::YouTube::new(hyper_client, auth);
        Ok(Self { client, user })
    }

    fn create_hyper_client() -> Client<HttpsConnector<HttpConnector>> {
        hyper::Client::builder().build(
            HttpsConnectorBuilder::new()
                .with_native_roots()
                .https_or_http()
                .enable_http1()
                .enable_http2()
                .build(),
        )
    }
}

enum TitleLocation {
    VideoTitle(usize),
    PlaylistTitle,
    Descriptions,
}
fn create_youtube_title(video: &VideosModel, target: TitleLocation) -> Result<String> {
    const YOUTUBE_TITLE_MAX_LENGTH: usize = 100;
    let max = video.part_count as usize;
    let date = parse_date(&video.created_at)
        .context(format!("could not parse date: {}", &video.created_at))?;
    let title = match target {
        TitleLocation::VideoTitle(current) => {
            let date_prefix = get_date_prefix(date.date());
            let part_prefix = if current != max {
                format_progress(max, current)
            } else {
                String::new()
            };
            shorten_string_if_needed(
                &format!("{}{} {}", date_prefix, part_prefix, video.name),
                YOUTUBE_TITLE_MAX_LENGTH,
            )
        }
        TitleLocation::PlaylistTitle => {
            let prefix = get_date_prefix(date.date());
            shorten_string_if_needed(
                &format!("{} {}", prefix, &video.name),
                YOUTUBE_TITLE_MAX_LENGTH,
            )
        }
        TitleLocation::Descriptions => format!("\"{}\"", video.name),
    };
    Ok(title)
}

fn format_progress(max: usize, current: usize) -> String {
    let width = (max.checked_ilog10().unwrap_or(0) + 1) as usize;
    format!("[{:0width$}/{:0width$}]", current, max, width = width)
}

const DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
fn parse_date(date: &str) -> ParseResult<NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(&date, DATETIME_FORMAT)
}

fn get_date_prefix(date: chrono::NaiveDate) -> String {
    format!(
        "[{:0>4}-{:0>2}-{:0>2}]",
        date.year(),
        date.month(),
        date.day()
    )
}

fn shorten_string_if_needed(s: &str, target_len: usize) -> String {
    const SHORTEN_CHARS: &str = "...";
    if target_len < SHORTEN_CHARS.len() {
        return SHORTEN_CHARS[..target_len].to_string();
    }
    if s.len() > target_len {
        let s = &s[..target_len - SHORTEN_CHARS.len()];
        let result = s.to_string() + SHORTEN_CHARS;
        assert_eq!(result.len(), target_len);
        result
    } else {
        s.to_string()
    }
}
#[cfg(test)]
mod test {
    use crate::client::youtube::{create_youtube_title, TitleLocation};
    use local_db::prelude::{Status, VideosModel};

    #[test]
    fn test_shorten_string() {
        let test = super::shorten_string_if_needed("123456789", 50);
        assert_eq!("123456789", test);
        let test = super::shorten_string_if_needed("123456789", 5);
        assert_eq!("12...", test);
        let test = super::shorten_string_if_needed("123456789", 3);
        assert_eq!("...", test);
        let test = super::shorten_string_if_needed("123456789", 2);
        assert_eq!("..", test);
        let test = super::shorten_string_if_needed("123456789", 0);
        assert_eq!("", test);
    }

    #[test]
    fn test_create_youtube_title() {
        let mut x = VideosModel {
            part_count: 4,
            name: "wow".to_string(),
            created_at: "2023-10-09T19:33:59".to_string(),
            //the rest is just dummy data
            id: 3,
            status: Status::Uploading,
            user_id: 0,
            twitch_id: String::new(),
            twitch_preview_image_url: None,
            twitch_download_url: None,
            duration: 0,
            youtube_id: None,
            youtube_playlist_name: String::new(),
            youtube_preview_image_url: None,
            youtube_playlist_id: None,
            youtube_playlist_created_at: None,
            fail_count: 0,
            fail_reason: None,
        };

        let description = create_youtube_title(&x, TitleLocation::Descriptions).unwrap();
        assert_eq!("\"wow\"", description);

        let playlist = create_youtube_title(&x, TitleLocation::PlaylistTitle).unwrap();
        assert_eq!("[2023-10-09] wow", playlist);

        let video = create_youtube_title(&x, TitleLocation::VideoTitle(2)).unwrap();
        assert_eq!("[2023-10-09][2/4] wow", video);

        x.part_count = 14;
        let video = create_youtube_title(&x, TitleLocation::VideoTitle(2)).unwrap();
        assert_eq!("[2023-10-09][02/14] wow", video);
    }
}
