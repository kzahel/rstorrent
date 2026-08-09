//! Pure geometry for recognizable published payload artifacts.

use std::error::Error;
use std::fmt;

use rstorrent_protocol::metainfo::{Metainfo, MetainfoMode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationShape {
    File,
    Tree,
}

impl PublicationShape {
    pub fn from_metainfo(metainfo: &Metainfo) -> Self {
        match metainfo.mode {
            MetainfoMode::SingleFile => Self::File,
            MetainfoMode::MultiFile => Self::Tree,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalPayloadArtifact {
    pub file_index: usize,
    pub components: Vec<String>,
    pub qualified_components: Vec<String>,
    pub length: u64,
    pub padding: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedArtifactLayout {
    pub namespace: String,
    pub shape: PublicationShape,
    pub files: Vec<LogicalPayloadArtifact>,
}

impl PublishedArtifactLayout {
    pub fn from_metainfo(metainfo: &Metainfo) -> Result<Self, ArtifactLayoutError> {
        Self::with_namespace(metainfo, metainfo.name.clone())
    }

    pub fn with_namespace(
        metainfo: &Metainfo,
        namespace: String,
    ) -> Result<Self, ArtifactLayoutError> {
        validate_component(&namespace)?;
        let shape = PublicationShape::from_metainfo(metainfo);
        if shape == PublicationShape::File && metainfo.files.len() != 1 {
            return Err(ArtifactLayoutError::InvalidSingleFileGeometry);
        }
        let mut files = Vec::with_capacity(metainfo.files.len());
        for (file_index, file) in metainfo.files.iter().enumerate() {
            if file.path.is_empty()
                || file
                    .path
                    .iter()
                    .any(|part| validate_component(part).is_err())
            {
                return Err(ArtifactLayoutError::InvalidComponent);
            }
            let qualified_components = match shape {
                PublicationShape::File => vec![namespace.clone()],
                PublicationShape::Tree => std::iter::once(namespace.clone())
                    .chain(file.path.iter().cloned())
                    .collect(),
            };
            files.push(LogicalPayloadArtifact {
                file_index,
                components: file.path.clone(),
                qualified_components,
                length: file.length,
                padding: file.padding,
            });
        }
        Ok(Self {
            namespace,
            shape,
            files,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactLayoutError {
    InvalidComponent,
    InvalidSingleFileGeometry,
}

impl fmt::Display for ArtifactLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidComponent => formatter.write_str("invalid artifact path component"),
            Self::InvalidSingleFileGeometry => {
                formatter.write_str("single-file artifact does not contain exactly one file")
            }
        }
    }
}

impl Error for ArtifactLayoutError {}

fn validate_component(component: &str) -> Result<(), ArtifactLayoutError> {
    if component.is_empty()
        || component.len() > 255
        || matches!(component, "." | "..")
        || component
            .bytes()
            .any(|byte| matches!(byte, 0 | b'/' | b'\\'))
    {
        return Err(ArtifactLayoutError::InvalidComponent);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};

    use super::{PublicationShape, PublishedArtifactLayout};

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
        let layout = PublishedArtifactLayout::from_metainfo(&single).expect("single layout");
        assert_eq!(layout.shape, PublicationShape::File);
        assert_eq!(layout.files[0].qualified_components, ["one.bin"]);

        let mut tree = single;
        tree.name = "tree".to_owned();
        tree.mode = MetainfoMode::MultiFile;
        tree.files[0].path = vec!["nested".to_owned(), "one.bin".to_owned()];
        let layout = PublishedArtifactLayout::from_metainfo(&tree).expect("tree layout");
        assert_eq!(layout.shape, PublicationShape::Tree);
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
        assert!(PublishedArtifactLayout::from_metainfo(&metainfo).is_err());
    }
}
