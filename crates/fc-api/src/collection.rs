//! Methods about the open collection (step 1.1a).

use fc_core::Core;
use fc_core::collection::CollectionError;
use serde::Serialize;
use ts_rs::TS;

use crate::{ApiError, ErrorKind, Method, OpContext};

impl From<CollectionError> for ApiError {
    fn from(error: CollectionError) -> Self {
        let kind = match &error {
            CollectionError::NotFound => ErrorKind::NotFound,
            CollectionError::AlreadyExists | CollectionError::NotACollection => {
                ErrorKind::InvalidInput
            }
            CollectionError::TooNew { .. } => ErrorKind::UpdateRequired,
            CollectionError::InUse => ErrorKind::Unavailable,
            CollectionError::Storage(_) => return Self::internal(),
        };
        Self::new(kind, error.to_string())
    }
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CollectionInfo {
    pub schema_version: u32,
    pub supported_schema_version: u32,
    pub created_by: String,
}

/// Facts about the open collection.
pub struct GetCollectionInfo;

impl Method for GetCollectionInfo {
    const NAME: &'static str = "getCollectionInfo";
    type Input = ();
    type Output = CollectionInfo;

    fn call(core: &Core, (): (), _: &OpContext) -> Result<CollectionInfo, ApiError> {
        let info = core
            .with_collection(|collection| collection.info())
            .ok_or_else(|| ApiError::new(ErrorKind::NotFound, "No collection is open."))??;
        Ok(CollectionInfo {
            schema_version: info.schema_version,
            supported_schema_version: info.supported_schema_version,
            created_by: info.created_by,
        })
    }
}
