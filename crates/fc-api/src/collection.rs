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

/// The error for a call that needs a collection when none is open. If the last attempt to open one
/// failed, it is that failure (a newer collection, a file in use), so the UI can explain it
/// instead of saying "No collection is open." (ADR 0010 decision 6).
pub(crate) fn not_open(core: &Core) -> ApiError {
    core.open_error().map_or_else(
        || ApiError::new(ErrorKind::NotFound, "No collection is open."),
        ApiError::from,
    )
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CollectionInfo {
    pub schema_version: u32,
    pub supported_schema_version: u32,
    pub created_by: String,
    /// This copy's device ID, a UUID string.
    pub device_id: String,
    /// Features the collection needs that this build does not know. Sync stays paused while there
    /// are any.
    pub unsupported_features: Vec<String>,
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
            .ok_or_else(|| not_open(core))??;
        Ok(CollectionInfo {
            schema_version: info.schema_version,
            supported_schema_version: info.supported_schema_version,
            created_by: info.created_by,
            device_id: info.device_id.to_string(),
            unsupported_features: info.unsupported_features,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fc_core::clock::{Host, ManualClock};
    use fc_core::id::Id;
    use serde_json::Value;

    use crate::{ErrorKind, OpContext, dispatch};

    use super::*;

    fn host() -> Host {
        Host {
            clock: Arc::new(ManualClock::new(1_700_000_000_000)),
            installation_id: Id::from_bytes([3; 16]),
        }
    }

    fn collection_info(core: &Core) -> Result<Value, ApiError> {
        dispatch(
            core,
            "getCollectionInfo",
            Value::Null,
            &OpContext::uncancellable(),
        )
        .map(|reply| reply.output)
    }

    /// A collection file saved by a newer app: `user_version` is the big-endian `u32` at byte 60
    /// of the SQLite header.
    fn newer_collection(path: &std::path::Path) {
        let core = Core::new();
        core.open_collection(path.to_str().unwrap(), host())
            .unwrap();
        core.close_collection().unwrap();
        let mut bytes = std::fs::read(path).unwrap();
        bytes[60..64].copy_from_slice(&99u32.to_be_bytes());
        std::fs::write(path, bytes).unwrap();
    }

    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("fc-api-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.join("collection.db"))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    #[test]
    fn a_collection_from_a_newer_app_answers_update_required() {
        let file = TempFile::new("newer");
        newer_collection(&file.0);
        let before = std::fs::read(&file.0).unwrap();

        let core = Core::new();
        let error = core
            .open_collection(file.0.to_str().unwrap(), host())
            .unwrap_err();
        assert!(matches!(error, CollectionError::TooNew { found: 99, .. }));

        let err = collection_info(&core).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UpdateRequired);
        assert!(err.message.contains("Update the app"), "{}", err.message);
        // The failed open left the file alone.
        assert_eq!(std::fs::read(&file.0).unwrap(), before);
    }

    #[test]
    fn without_a_failed_open_the_error_is_still_no_collection() {
        let err = collection_info(&Core::new()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(err.message, "No collection is open.");
    }

    #[test]
    fn a_later_good_open_clears_the_remembered_error() {
        let file = TempFile::new("recovers");
        newer_collection(&file.0);
        let core = Core::new();
        core.open_collection(file.0.to_str().unwrap(), host())
            .unwrap_err();
        assert!(core.open_error().is_some());

        core.open_collection(":memory:", host()).unwrap();
        assert!(core.open_error().is_none());
        assert!(collection_info(&core).is_ok());
    }

    #[test]
    fn closing_on_purpose_forgets_the_error() {
        let file = TempFile::new("closed");
        newer_collection(&file.0);
        let core = Core::new();
        core.open_collection(file.0.to_str().unwrap(), host())
            .unwrap_err();
        core.close_collection().unwrap();
        assert_eq!(
            collection_info(&core).unwrap_err().kind,
            ErrorKind::NotFound
        );
    }
}
