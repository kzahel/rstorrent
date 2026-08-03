use rstorrent_protocol::magnet::{MAX_MAGNET_LENGTH, Magnet};
use rstorrent_protocol::metainfo::MAX_FILES;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const CONTROL_VERSION: u16 = 1;
pub const MAX_REQUEST_ID_LENGTH: usize = 128;
pub const MAX_PROFILE_ID_LENGTH: usize = 128;
pub const MAX_ROOT_ID_LENGTH: usize = 128;
pub const MAX_ROOT_LABEL_LENGTH: usize = 256;
pub const MAX_ERROR_MESSAGE_LENGTH: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RequestEnvelope {
    pub version: u16,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    pub command: Command,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    AddMagnet {
        magnet: String,
        storage_root: String,
        #[serde(default = "default_true")]
        start_content: bool,
        #[serde(default)]
        skip_files: Vec<u32>,
    },
    SetFilePriority {
        torrent_id: String,
        file_indices: Vec<u32>,
        priority: FilePriority,
    },
    SetDefaultStorageRoot {
        storage_root: String,
    },
    SetShowAddOptions {
        show: bool,
    },
    RemoveStorageRoot {
        storage_root: String,
    },
    Snapshot,
    Pause {
        torrent_id: String,
    },
    Resume {
        torrent_id: String,
    },
    Archive {
        torrent_id: String,
    },
    RestoreArchive {
        torrent_id: String,
    },
    RemoveTorrent {
        torrent_id: String,
        data: RemovalDataPolicy,
    },
    Shutdown,
}

impl Command {
    pub(crate) fn is_mutation(&self) -> bool {
        matches!(
            self,
            Self::AddMagnet { .. }
                | Self::SetDefaultStorageRoot { .. }
                | Self::SetShowAddOptions { .. }
                | Self::RemoveStorageRoot { .. }
                | Self::Pause { .. }
                | Self::Resume { .. }
                | Self::SetFilePriority { .. }
                | Self::Archive { .. }
                | Self::RestoreArchive { .. }
                | Self::RemoveTorrent { .. }
        )
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum FilePriority {
    Normal,
    Skip,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum RemovalDataPolicy {
    Keep,
    DeleteManaged,
}

impl RemovalDataPolicy {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::DeleteManaged => "delete_managed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "keep" => Some(Self::Keep),
            "delete_managed" => Some(Self::DeleteManaged),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum RemovalState {
    Pending,
    AwaitingPlatform,
    Failed,
}

impl RemovalState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::AwaitingPlatform => "awaiting_platform",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "awaiting_platform" => Some(Self::AwaitingPlatform),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ResponseEnvelope {
    pub version: u16,
    pub request_id: String,
    pub revision: String,
    #[serde(flatten)]
    pub outcome: ResponseOutcome,
}

impl ResponseEnvelope {
    pub fn success(request_id: String, revision: u64, snapshot: ServiceSnapshot) -> Self {
        Self {
            version: CONTROL_VERSION,
            request_id,
            revision: revision.to_string(),
            outcome: ResponseOutcome::Success { snapshot },
        }
    }

    pub fn error(
        request_id: String,
        revision: u64,
        code: ErrorCode,
        message: impl Into<String>,
    ) -> Self {
        let mut message = message.into();
        if message.len() > MAX_ERROR_MESSAGE_LENGTH {
            let mut boundary = MAX_ERROR_MESSAGE_LENGTH;
            while !message.is_char_boundary(boundary) {
                boundary -= 1;
            }
            message.truncate(boundary);
        }
        Self {
            version: CONTROL_VERSION,
            request_id,
            revision: revision.to_string(),
            outcome: ResponseOutcome::Error {
                error: ErrorResponse { code, message },
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseOutcome {
    Success { snapshot: ServiceSnapshot },
    Error { error: ErrorResponse },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidVersion,
    InvalidRequest,
    RequestConflict,
    StaleRevision,
    UnknownStorageRoot,
    StorageRootInUse,
    UnknownTorrent,
    InvalidTorrentState,
    InvalidDurableState,
    StorageNeedsRepair,
    Busy,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ServiceSnapshot {
    pub profile_id: String,
    pub revision: String,
    #[serde(default)]
    pub storage: StorageSettingsSnapshot,
    pub torrents: Vec<TorrentSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StorageSettingsSnapshot {
    pub roots: Vec<StorageRootSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_root: Option<String>,
    pub show_add_options: bool,
}

impl Default for StorageSettingsSnapshot {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            default_root: None,
            show_add_options: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StorageRootSnapshot {
    pub root_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_path: Option<String>,
    pub availability: StorageRootAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum StorageRootAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TorrentSnapshot {
    pub torrent_id: String,
    pub storage_root: String,
    pub state: TorrentState,
    pub storage_state: StorageState,
    pub metadata_available: bool,
    pub piece_count: u32,
    pub verified_piece_count: u32,
    pub skip_files: Vec<u32>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removal_state: Option<RemovalState>,
    #[serde(default)]
    pub delete_managed_data_supported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum TorrentState {
    AwaitingMetadata,
    AwaitingStorage,
    Checking,
    Downloading,
    AwaitingPublication,
    Paused,
    Complete,
    NeedsRepair,
    Error,
}

impl TorrentState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingMetadata => "awaiting_metadata",
            Self::AwaitingStorage => "awaiting_storage",
            Self::Checking => "checking",
            Self::Downloading => "downloading",
            Self::AwaitingPublication => "awaiting_publication",
            Self::Paused => "paused",
            Self::Complete => "complete",
            Self::NeedsRepair => "needs_repair",
            Self::Error => "error",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "awaiting_metadata" => Some(Self::AwaitingMetadata),
            "awaiting_storage" => Some(Self::AwaitingStorage),
            "checking" => Some(Self::Checking),
            "downloading" => Some(Self::Downloading),
            "awaiting_publication" => Some(Self::AwaitingPublication),
            "paused" => Some(Self::Paused),
            "complete" => Some(Self::Complete),
            "needs_repair" => Some(Self::NeedsRepair),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum StorageState {
    None,
    Staging,
    Prepared,
    Published,
    NeedsRepair,
}

impl StorageState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Staging => "staging",
            Self::Prepared => "prepared",
            Self::Published => "published",
            Self::NeedsRepair => "needs_repair",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "staging" => Some(Self::Staging),
            "prepared" => Some(Self::Prepared),
            "published" => Some(Self::Published),
            "needs_repair" => Some(Self::NeedsRepair),
            _ => None,
        }
    }
}

pub(crate) fn validate_request(request: &RequestEnvelope) -> Result<(), (ErrorCode, String)> {
    if request.version != CONTROL_VERSION {
        return Err((
            ErrorCode::InvalidVersion,
            format!(
                "control version {} is unsupported; expected {CONTROL_VERSION}",
                request.version
            ),
        ));
    }
    validate_identifier(&request.request_id, "request ID", MAX_REQUEST_ID_LENGTH)?;
    if let Some(revision) = &request.expected_revision {
        parse_revision(revision)?;
    }
    match &request.command {
        Command::AddMagnet {
            magnet,
            storage_root,
            start_content: _,
            skip_files,
        } => {
            if magnet.len() > MAX_MAGNET_LENGTH {
                return Err((
                    ErrorCode::InvalidRequest,
                    format!("magnet exceeds {MAX_MAGNET_LENGTH} bytes"),
                ));
            }
            Magnet::parse(magnet)
                .map_err(|error| (ErrorCode::InvalidRequest, error.to_string()))?;
            validate_identifier(storage_root, "storage root", MAX_ROOT_ID_LENGTH)?;
            if skip_files.len() > MAX_FILES {
                return Err((
                    ErrorCode::InvalidRequest,
                    format!("file selection exceeds {MAX_FILES} entries"),
                ));
            }
            let mut previous = None;
            for index in skip_files {
                if usize::try_from(*index).map_or(true, |index| index >= MAX_FILES) {
                    return Err((
                        ErrorCode::InvalidRequest,
                        "file selection index exceeds the supported file bound".to_owned(),
                    ));
                }
                if previous.is_some_and(|previous| previous >= *index) {
                    return Err((
                        ErrorCode::InvalidRequest,
                        "file selection indices must be sorted and unique".to_owned(),
                    ));
                }
                previous = Some(*index);
            }
        }
        Command::SetFilePriority {
            torrent_id,
            file_indices,
            priority: _,
        } => {
            validate_torrent_id(torrent_id)?;
            if file_indices.is_empty() || file_indices.len() > MAX_FILES {
                return Err((
                    ErrorCode::InvalidRequest,
                    format!("file selection must contain between 1 and {MAX_FILES} entries"),
                ));
            }
            let mut previous = None;
            for index in file_indices {
                if usize::try_from(*index).map_or(true, |index| index >= MAX_FILES) {
                    return Err((
                        ErrorCode::InvalidRequest,
                        "file selection index exceeds the supported file bound".to_owned(),
                    ));
                }
                if previous.is_some_and(|previous| previous >= *index) {
                    return Err((
                        ErrorCode::InvalidRequest,
                        "file selection indices must be sorted and unique".to_owned(),
                    ));
                }
                previous = Some(*index);
            }
        }
        Command::Pause { torrent_id }
        | Command::Resume { torrent_id }
        | Command::Archive { torrent_id }
        | Command::RestoreArchive { torrent_id }
        | Command::RemoveTorrent { torrent_id, .. } => {
            validate_torrent_id(torrent_id)?;
        }
        Command::SetDefaultStorageRoot { storage_root }
        | Command::RemoveStorageRoot { storage_root } => {
            validate_identifier(storage_root, "storage root", MAX_ROOT_ID_LENGTH)?;
        }
        Command::SetShowAddOptions { .. } => {}
        Command::Snapshot | Command::Shutdown => {}
    }
    Ok(())
}

pub(crate) fn parse_revision(value: &str) -> Result<u64, (ErrorCode, String)> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err((
            ErrorCode::InvalidRequest,
            "revision must be a canonical unsigned decimal string".to_owned(),
        ));
    }
    value.parse().map_err(|_| {
        (
            ErrorCode::InvalidRequest,
            "revision exceeds the supported unsigned 64-bit range".to_owned(),
        )
    })
}

pub(crate) fn validate_identifier(
    value: &str,
    label: &str,
    maximum: usize,
) -> Result<(), (ErrorCode, String)> {
    if value.is_empty()
        || value.len() > maximum
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err((
            ErrorCode::InvalidRequest,
            format!("{label} must be 1..={maximum} ASCII letters, digits, '.', '-', or '_'"),
        ));
    }
    Ok(())
}

pub(crate) fn validate_torrent_id(value: &str) -> Result<(), (ErrorCode, String)> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err((
            ErrorCode::InvalidRequest,
            "torrent ID must be a 40-character hexadecimal v1 info hash".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn encode_info_hash(info_hash: [u8; 20]) -> String {
    let mut output = String::with_capacity(40);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in info_hash {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(crate) fn decode_info_hash(value: &str) -> Option<[u8; 20]> {
    if value.len() != 40 {
        return None;
    }
    let mut output = [0_u8; 20];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(output)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CONTROL_VERSION, Command, ErrorCode, FilePriority, RequestEnvelope, encode_info_hash,
        validate_request,
    };

    #[test]
    fn validates_sorted_bounded_selection() {
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "request-1".to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet:
                    "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213&x.pe=127.0.0.1:1"
                        .to_owned(),
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: vec![1, 3],
            },
        };
        assert_eq!(validate_request(&request), Ok(()));

        let mut invalid = request;
        if let Command::AddMagnet { skip_files, .. } = &mut invalid.command {
            *skip_files = vec![3, 1];
        }
        assert_eq!(
            validate_request(&invalid).map_err(|error| error.0),
            Err(ErrorCode::InvalidRequest)
        );
    }

    #[test]
    fn add_defaults_to_starting_content_and_file_priority_is_bounded() {
        let add: RequestEnvelope = serde_json::from_str(
            r#"{"version":1,"request_id":"add","command":{"type":"add_magnet","magnet":"magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213","storage_root":"downloads"}}"#,
        )
        .expect("decode legacy add envelope");
        assert!(matches!(
            add.command,
            Command::AddMagnet {
                start_content: true,
                ..
            }
        ));

        let mut request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "priority".to_owned(),
            expected_revision: None,
            command: Command::SetFilePriority {
                torrent_id: "000102030405060708090a0b0c0d0e0f10111213".to_owned(),
                file_indices: vec![1, 3],
                priority: FilePriority::Skip,
            },
        };
        assert_eq!(validate_request(&request), Ok(()));
        if let Command::SetFilePriority { file_indices, .. } = &mut request.command {
            *file_indices = vec![3, 1];
        }
        assert!(validate_request(&request).is_err());
        if let Command::SetFilePriority { file_indices, .. } = &mut request.command {
            file_indices.clear();
        }
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn info_hash_identifier_is_lowercase_hex() {
        assert_eq!(
            encode_info_hash([
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff, 0x10, 0x11, 0x12, 0x13,
            ]),
            "00010203040506070809aabbccddeeff10111213"
        );
    }

    #[test]
    fn revisions_are_canonical_decimal_strings() {
        let mut request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "request-1".to_owned(),
            expected_revision: Some("18446744073709551615".to_owned()),
            command: Command::Snapshot,
        };
        assert_eq!(validate_request(&request), Ok(()));
        for invalid in ["", "01", "-1", "18446744073709551616"] {
            request.expected_revision = Some(invalid.to_owned());
            assert_eq!(
                validate_request(&request).map_err(|error| error.0),
                Err(ErrorCode::InvalidRequest)
            );
        }
    }
}
