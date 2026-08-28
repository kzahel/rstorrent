use std::collections::BTreeMap;

use rstorrent_media_catalog::{
    EpisodeClassification, MediaClassification, classify_video, video_extension,
};
use rstorrent_protocol::storage_layout::ContentLayout;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::file_views::{FileSelectionView, FileView};
use crate::media::MediaFileAvailability;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum MediaCatalogState {
    MetadataPending,
    Available,
    TorrentMissing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaRoleView {
    Episode {
        series_title_hint: String,
        season_number: u16,
        episode_number: u16,
        ending_episode_number: Option<u16>,
    },
    UnclassifiedVideo,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MediaItemView {
    pub media_id: String,
    pub file_index: u32,
    pub path: Vec<String>,
    pub extension: String,
    pub length_bytes: String,
    pub selection: FileSelectionView,
    pub done_bytes: String,
    pub verified_bytes: String,
    pub media_availability: MediaFileAvailability,
    pub role: MediaRoleView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DerivedMediaCatalog {
    entries: Vec<DerivedMediaEntry>,
    by_file_index: BTreeMap<usize, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DerivedMediaEntry {
    file_index: usize,
    extension: String,
    role: MediaRoleView,
}

impl DerivedMediaCatalog {
    pub(crate) fn from_layout(layout: &ContentLayout) -> Self {
        let mut entries = Vec::new();
        let mut by_file_index = BTreeMap::new();
        for (file_index, file) in layout.files().iter().enumerate() {
            if file.padding {
                continue;
            }
            let Some(classification) = classify_video(&file.path) else {
                continue;
            };
            let filename = file
                .path
                .last()
                .expect("validated content paths retain a filename");
            let extension = video_extension(filename)
                .expect("classified video retains its recognized extension")
                .to_owned();
            let role = match classification {
                MediaClassification::Episode(EpisodeClassification {
                    series_title_hint,
                    season_number,
                    episode_number,
                    ending_episode_number,
                }) => MediaRoleView::Episode {
                    series_title_hint,
                    season_number,
                    episode_number,
                    ending_episode_number,
                },
                MediaClassification::UnclassifiedVideo => MediaRoleView::UnclassifiedVideo,
            };
            by_file_index.insert(file_index, entries.len());
            entries.push(DerivedMediaEntry {
                file_index,
                extension,
                role,
            });
        }
        Self {
            entries,
            by_file_index,
        }
    }

    pub(crate) fn entries(&self) -> &[DerivedMediaEntry] {
        &self.entries
    }

    pub(crate) fn entry(&self, file_index: usize) -> Option<&DerivedMediaEntry> {
        self.by_file_index
            .get(&file_index)
            .and_then(|index| self.entries.get(*index))
    }
}

impl DerivedMediaEntry {
    pub(crate) fn file_index(&self) -> usize {
        self.file_index
    }

    pub(crate) fn view(&self, file: FileView) -> MediaItemView {
        MediaItemView {
            media_id: file.file_id,
            file_index: file.file_index,
            path: file.path,
            extension: self.extension.clone(),
            length_bytes: file.length_bytes,
            selection: file
                .selection
                .expect("derived media never includes padding files"),
            done_bytes: file.done_bytes,
            verified_bytes: file.verified_bytes,
            media_availability: file.media_availability,
            role: self.role.clone(),
        }
    }
}
