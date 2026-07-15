use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Asset;

pub(crate) const MANIFEST_FILE_NAME: &str = ".mdviewer-assets.json";
pub(crate) const SCHEMA_VERSION: &str = "mdviewer.assets/v1";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetManifest {
    pub(crate) schema_version: String,
    pub(crate) document: String,
    pub(crate) assets: Vec<ManifestAsset>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestAsset {
    pub(crate) file_name: String,
    pub(crate) media_type: String,
    pub(crate) sha256: String,
}

impl AssetManifest {
    pub(crate) fn new(document: &str, assets: &[Asset]) -> Self {
        let mut entries = assets
            .iter()
            .map(|asset| ManifestAsset {
                file_name: asset.file_name.clone(),
                media_type: asset.media_type.clone(),
                sha256: sha256_hex(&asset.data),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.file_name.cmp(&right.file_name));

        Self {
            schema_version: SCHEMA_VERSION.into(),
            document: document.into(),
            assets: entries,
        }
    }
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
