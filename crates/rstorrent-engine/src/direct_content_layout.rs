//! Pure geometry for direct final-path torrent content.

use std::error::Error;
use std::fmt;

use rstorrent_protocol::content::TorrentContent;
use rstorrent_protocol::metainfo::{Metainfo, MetainfoMode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentShape {
    File,
    Tree,
}

impl ContentShape {
    pub fn from_metainfo(metainfo: &Metainfo) -> Self {
        match metainfo.mode {
            MetainfoMode::SingleFile => Self::File,
            MetainfoMode::MultiFile => Self::Tree,
        }
    }

    pub fn from_content(content: &TorrentContent) -> Self {
        match content {
            TorrentContent::V1(content) => Self::from_metainfo(&content.metainfo),
            TorrentContent::V2(_) | TorrentContent::Hybrid(_) => {
                let metainfo = content
                    .v2_metainfo()
                    .expect("v2-shaped content has v2 metainfo");
                if metainfo.files.len() == 1 && metainfo.files[0].path == [metainfo.name.as_str()] {
                    Self::File
                } else {
                    Self::Tree
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalContentFile {
    pub file_index: usize,
    pub components: Vec<String>,
    pub qualified_components: Vec<String>,
    pub length: u64,
    pub padding: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectContentLayout {
    pub root_name: String,
    pub shape: ContentShape,
    pub files: Vec<LogicalContentFile>,
}

impl DirectContentLayout {
    pub fn from_metainfo(metainfo: &Metainfo) -> Result<Self, DirectContentLayoutError> {
        Self::with_root_name(metainfo, metainfo.name.clone())
    }

    pub fn from_content(content: &TorrentContent) -> Result<Self, DirectContentLayoutError> {
        match content {
            TorrentContent::V1(content) => Self::from_metainfo(&content.metainfo),
            TorrentContent::V2(_) | TorrentContent::Hybrid(_) => {
                let metainfo = content
                    .v2_metainfo()
                    .expect("v2-shaped content has v2 metainfo");
                validate_component(&metainfo.name)?;
                let shape = if metainfo.files.len() == 1
                    && metainfo.files[0].path == [metainfo.name.as_str()]
                {
                    ContentShape::File
                } else {
                    ContentShape::Tree
                };
                let mut files = Vec::with_capacity(metainfo.files.len());
                for (file_index, file) in metainfo.files.iter().enumerate() {
                    if file.path.is_empty()
                        || file
                            .path
                            .iter()
                            .any(|part| validate_component(part).is_err())
                    {
                        return Err(DirectContentLayoutError::InvalidComponent);
                    }
                    let qualified_components = match shape {
                        ContentShape::File => vec![metainfo.name.clone()],
                        ContentShape::Tree => std::iter::once(metainfo.name.clone())
                            .chain(file.path.iter().cloned())
                            .collect(),
                    };
                    files.push(LogicalContentFile {
                        file_index,
                        components: file.path.clone(),
                        qualified_components,
                        length: file.length,
                        padding: false,
                    });
                }
                Ok(Self {
                    root_name: metainfo.name.clone(),
                    shape,
                    files,
                })
            }
        }
    }

    pub fn with_root_name(
        metainfo: &Metainfo,
        root_name: String,
    ) -> Result<Self, DirectContentLayoutError> {
        validate_component(&root_name)?;
        let shape = ContentShape::from_metainfo(metainfo);
        if shape == ContentShape::File && metainfo.files.len() != 1 {
            return Err(DirectContentLayoutError::InvalidSingleFileGeometry);
        }
        let mut files = Vec::with_capacity(metainfo.files.len());
        for (file_index, file) in metainfo.files.iter().enumerate() {
            if file.path.is_empty()
                || file
                    .path
                    .iter()
                    .any(|part| validate_component(part).is_err())
            {
                return Err(DirectContentLayoutError::InvalidComponent);
            }
            let qualified_components = match shape {
                ContentShape::File => vec![root_name.clone()],
                ContentShape::Tree => std::iter::once(root_name.clone())
                    .chain(file.path.iter().cloned())
                    .collect(),
            };
            files.push(LogicalContentFile {
                file_index,
                components: file.path.clone(),
                qualified_components,
                length: file.length,
                padding: file.padding,
            });
        }
        Ok(Self {
            root_name,
            shape,
            files,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectContentLayoutError {
    InvalidComponent,
    InvalidSingleFileGeometry,
}

impl fmt::Display for DirectContentLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidComponent => formatter.write_str("invalid artifact path component"),
            Self::InvalidSingleFileGeometry => {
                formatter.write_str("single-file artifact does not contain exactly one file")
            }
        }
    }
}

impl Error for DirectContentLayoutError {}

fn validate_component(component: &str) -> Result<(), DirectContentLayoutError> {
    if component.is_empty()
        || component.len() > 255
        || matches!(component, "." | "..")
        || component
            .bytes()
            .any(|byte| matches!(byte, 0 | b'/' | b'\\'))
    {
        return Err(DirectContentLayoutError::InvalidComponent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};

    use super::{ContentShape, DirectContentLayout};

    #[test]
    fn maps_single_and_tree_files_to_safe_qualified_components() {
        let single = Metainfo {
            info_hash: [1; 20],
            piece_hashes: vec![[2; 20]],
            piece_length: 4,
            total_length: 4,
            name: "one.bin".to_owned(),
            private: false,
            mode: MetainfoMode::SingleFile,
            files: vec![MetainfoFile {
                path: vec!["one.bin".to_owned()],
                length: 4,
                offset: 0,
                padding: false,
            }],
        };
        let layout = DirectContentLayout::from_metainfo(&single).expect("single layout");
        assert_eq!(layout.shape, ContentShape::File);
        assert_eq!(layout.files[0].qualified_components, ["one.bin"]);

        let mut tree = single;
        tree.name = "tree".to_owned();
        tree.mode = MetainfoMode::MultiFile;
        tree.files[0].path = vec!["nested".to_owned(), "one.bin".to_owned()];
        let layout = DirectContentLayout::from_metainfo(&tree).expect("tree layout");
        assert_eq!(layout.shape, ContentShape::Tree);
        assert_eq!(
            layout.files[0].qualified_components,
            ["tree", "nested", "one.bin"]
        );
    }

    #[test]
    fn rejects_unsafe_geometry_without_touching_a_backend() {
        let metainfo = Metainfo {
            info_hash: [1; 20],
            piece_hashes: vec![[2; 20]],
            piece_length: 4,
            total_length: 4,
            name: "tree".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files: vec![MetainfoFile {
                path: vec!["..".to_owned(), "one.bin".to_owned()],
                length: 4,
                offset: 0,
                padding: false,
            }],
        };
        assert!(DirectContentLayout::from_metainfo(&metainfo).is_err());
    }
}
