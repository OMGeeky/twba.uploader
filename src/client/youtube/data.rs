use crate::client::youtube::data::substitutions::*;
use crate::prelude::*;
use crate::CONF;
use chrono::{Datelike, NaiveDateTime, ParseResult};
use google_youtube3::api::enums::{PlaylistStatuPrivacyStatusEnum, VideoStatuPrivacyStatusEnum};
use std::fmt::Debug;
use twba_local_db::prelude::{UsersModel, VideosModel};

/// The maximum length of a YouTube title that is allowed
///
/// This is a constant because it is a hard limit set by YouTube
const YOUTUBE_TITLE_MAX_LENGTH: usize = 100;
pub mod substitutions {
    pub const ORIGINAL_TITLE: &str = "$$original_title$$";
    pub const ORIGINAL_DESCRIPTION: &str = "$$original_description$$";
    pub const UPLOAD_DATE: &str = "$$upload_date$$";
    pub const UPLOAD_DATE_SHORT: &str = "$$upload_date_short$$";
    pub const TWITCH_URL: &str = "$$twitch_url$$";
    pub const TWITCH_CHANNEL_NAME: &str = "$$twitch_channel_name$$";
    pub const TWITCH_CHANNEL_URL: &str = "$$twitch_channel_url$$";
    pub const PART_COUNT: &str = "$$part_count$$";
    pub const PART_IDENT: &str = "$$part_ident$$";
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Location {
    Video(usize),
    Playlist,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VideoData {
    pub part_number: usize,
    pub video_title: String,
    pub video_description: String,
    pub video_tags: Vec<String>,
    pub video_category: u32,
    pub video_privacy: VideoStatuPrivacyStatusEnum,
    pub playlist_title: String,
    pub playlist_description: String,
    pub playlist_privacy: PlaylistStatuPrivacyStatusEnum,
}
pub struct Templates {
    pub video_title: String,
    pub video_description: String,
    pub playlist_title: String,
    pub playlist_description: String,
}
impl Default for Templates {
    fn default() -> Self {
        Self {
            video_title: format!("[{}]{} {}", UPLOAD_DATE_SHORT, PART_IDENT, ORIGINAL_TITLE),
            video_description: format!(
                "default description for video: {} from {}\n\nOriginal stream here: \n{}\n\nWatch {} live at: {}",
                ORIGINAL_TITLE, UPLOAD_DATE, TWITCH_URL, TWITCH_CHANNEL_NAME, TWITCH_CHANNEL_URL
            ),
            playlist_title: format!("[{}] {}", UPLOAD_DATE_SHORT, ORIGINAL_TITLE),
            playlist_description: format!(
                "default description for video: {} from {}\n\nOriginal stream here: \n{}\n\nWatch {} live at: {}",
                ORIGINAL_TITLE, UPLOAD_DATE, TWITCH_URL, TWITCH_CHANNEL_NAME, TWITCH_CHANNEL_URL
            ),
        }
    }
}

pub(crate) fn create_youtube_description(
    video: &VideosModel,
    user: &UsersModel,
    target: Location,
) -> Result<String> {
    let s = get_description_template(target);
    let description = substitute(s, video, user, target)?;
    Ok(description)
}
pub(crate) fn create_youtube_title(
    video: &VideosModel,
    user: &UsersModel,
    target: Location,
) -> Result<String> {
    let title_template = get_title_template(target);
    let title = substitute(title_template, video, user, target)?;
    let max_len = match target {
        Location::Video(_) => Some(YOUTUBE_TITLE_MAX_LENGTH),
        Location::Playlist => Some(YOUTUBE_TITLE_MAX_LENGTH),
        Location::Other => None,
    };
    let title = shorten_string_if_needed(&title, max_len);
    Ok(title)
}

fn get_title_template(target: Location) -> String {
    let templates = Templates::default();
    match target {
        Location::Video(_) => templates.video_title,
        Location::Playlist => templates.playlist_title,
        Location::Other => format!("\"{}\"", ORIGINAL_TITLE),
    }
}
fn get_description_template(target: Location) -> String {
    let configured = &CONF.google.youtube.default_description_template;
    if !configured.is_empty() {
        return configured.to_string();
    }
    let templates = Templates::default();
    match target {
        Location::Video(_) => templates.video_description,
        Location::Playlist => templates.playlist_description,
        Location::Other => templates.video_description,
    }
}

fn substitute(
    input: String,
    video: &VideosModel,
    user: &UsersModel,
    target: Location,
) -> Result<String> {
    let max = video.part_count as usize;
    let s = substitute_common(input, video, user, max)?;

    let title = match target {
        Location::Video(current) => substitute_part_ident(&s, current, max),
        _ => s,
    };
    Ok(title)
}
fn substitute_part_ident(input: &str, current: usize, max: usize) -> String {
    let part_prefix = if max > 1 {
        format_progress(max, current)
    } else {
        String::new()
    };
    input.replace(PART_IDENT, &part_prefix)
}
fn substitute_common(
    input: String,
    video: &VideosModel,
    user: &UsersModel,
    max: usize,
) -> Result<String> {
    let date = parse_date(&video.created_at).map_err(UploaderError::ParseDate)?;
    let date_prefix = get_date_prefix(date.date());
    Ok(input
        .replace(ORIGINAL_TITLE, &video.name)
        .replace(ORIGINAL_DESCRIPTION, "")
        .replace(UPLOAD_DATE, &date.to_string())
        .replace(UPLOAD_DATE_SHORT, &date_prefix)
        .replace(
            TWITCH_URL,
            video.twitch_download_url.as_ref().unwrap_or(&String::new()),
        )
        .replace(TWITCH_CHANNEL_NAME, &user.twitch_name)
        .replace(
            TWITCH_CHANNEL_URL,
            &format!("https://twitch.tv/{}", &user.twitch_id),
        )
        .replace(PART_COUNT, &max.to_string()))
}

fn shorten_string_if_needed(s: &str, target_len: Option<usize>) -> String {
    const SHORTEN_CHARS: &str = "...";
    if target_len.is_none() {
        return s.to_string();
    }
    let target_len = target_len.unwrap();
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
fn get_date_prefix(date: chrono::NaiveDate) -> String {
    format!(
        "{:0>4}-{:0>2}-{:0>2}",
        date.year(),
        date.month(),
        date.day()
    )
}

fn format_progress(max: usize, current: usize) -> String {
    let width = (max.checked_ilog10().unwrap_or(0) + 1) as usize;
    format!("[{:0width$}/{:0width$}]", current, max, width = width)
}

fn parse_date(date: &str) -> ParseResult<NaiveDateTime> {
    Ok(chrono::DateTime::parse_from_rfc3339(date)?.naive_local())
}

#[cfg(test)]
mod test {
    use crate::client::youtube::data::create_youtube_title;
    use crate::client::youtube::data::Location;
    use twba_local_db::prelude::{Status, UsersModel, VideosModel};

    #[test]
    fn test_shorten_string() {
        let test = super::shorten_string_if_needed("123456789", Some(50));
        assert_eq!("123456789", test);
        let test = super::shorten_string_if_needed("123456789", Some(5));
        assert_eq!("12...", test);
        let test = super::shorten_string_if_needed("123456789", Some(3));
        assert_eq!("...", test);
        let test = super::shorten_string_if_needed("123456789", Some(2));
        assert_eq!("..", test);
        let test = super::shorten_string_if_needed("123456789", Some(0));
        assert_eq!("", test);
        let test = super::shorten_string_if_needed("123456789", None);
        assert_eq!("123456789", test);
    }

    #[test]
    fn test_create_youtube_title_other() {
        let (mut x, user) = get_test_sample_data();
        let description = create_youtube_title(&x, &user, Location::Other).unwrap();
        assert_eq!("\"wow\"", description);
    }

    #[test]
    fn test_create_youtube_title_playlist() {
        let (mut x, user) = get_test_sample_data();
        let playlist = create_youtube_title(&x, &user, Location::Playlist).unwrap();
        assert_eq!("[2023-10-09] wow", playlist);
    }
    #[test]
    fn test_create_youtube_title_video_1() {
        let (mut x, user) = get_test_sample_data();
        let video = create_youtube_title(&x, &user, Location::Video(1)).unwrap();
        assert_eq!("[2023-10-09][1/4] wow", video);
    }
    #[test]
    fn test_create_youtube_title_video_2() {
        let (mut x, user) = get_test_sample_data();
        let video = create_youtube_title(&x, &user, Location::Video(2)).unwrap();
        assert_eq!("[2023-10-09][2/4] wow", video);
    }
    #[test]
    fn test_create_youtube_title_video_3() {
        let (mut x, user) = get_test_sample_data();
        let video = create_youtube_title(&x, &user, Location::Video(3)).unwrap();
        assert_eq!("[2023-10-09][3/4] wow", video);
    }
    #[test]
    fn test_create_youtube_title_video_4() {
        let (mut x, user) = get_test_sample_data();
        let video = create_youtube_title(&x, &user, Location::Video(4)).unwrap();
        assert_eq!("[2023-10-09][4/4] wow", video);
    }
    #[test]
    fn test_create_youtube_title_video_multi_digit_part_count() {
        let (mut x, user) = get_test_sample_data();

        x.part_count = 14;
        let video = create_youtube_title(&x, &user, Location::Video(2)).unwrap();
        assert_eq!("[2023-10-09][02/14] wow", video);
    }

    fn get_test_sample_data() -> (VideosModel, UsersModel) {
        let mut x = VideosModel {
            part_count: 4,
            name: "wow".to_string(),
            created_at: "2023-10-09T19:33:59+00:00".to_string(),
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
        let user = UsersModel {
            id: 0,
            twitch_id: "".to_string(),
            twitch_name: "".to_string(),
            twitch_profile_image_url: None,
            youtube_id: "".to_string(),
            youtube_name: "".to_string(),
            youtube_profile_image_url: None,
            youtube_target_duration: 0,
            youtube_max_duration: 0,
            active: false,
        };
        (x, user)
    }
}
