//! The core's public API: a list of methods, one dispatcher for every host (Tauri, web, CLI), and
//! the TypeScript bindings generated from the same list. No Tauri or platform dependency.
//! See `docs/adr/0002-ui-core-bridge.md`.

mod bindings;
mod context;
mod error;
mod examples;

pub use bindings::generate_bindings;
pub use context::OpContext;
pub use error::{ApiError, ErrorKind};
pub use fc_core::Core;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use ts_rs::TS;

/// One API method. Implement it, then add the type to the `methods!` list below.
pub trait Method {
    /// camelCase name used on the wire.
    const NAME: &'static str;
    type Input: DeserializeOwned + TS + 'static;
    type Output: Serialize + TS + 'static;

    fn call(core: &Core, input: Self::Input, ctx: &OpContext) -> Result<Self::Output, ApiError>;
}

fn run<M: Method>(core: &Core, input: Value, ctx: &OpContext) -> Result<Value, ApiError> {
    let input: M::Input = serde_json::from_value(input).map_err(|e| {
        ApiError::invalid_input(format!(
            "The app sent a request to \"{}\" that this version could not read ({e}). Update the app and try again.",
            M::NAME
        ))
    })?;
    let output = M::call(core, input, ctx)?;
    serde_json::to_value(output).map_err(|_| ApiError::internal())
}

/// Registers every method: generates `dispatch` and the TypeScript `Methods` map from one list.
macro_rules! methods {
    ($($m:ty),* $(,)?) => {
        /// Runs the named method. Every host calls this.
        pub fn dispatch(
            core: &Core,
            method: &str,
            input: Value,
            ctx: &OpContext,
        ) -> Result<Value, ApiError> {
            $( if method == <$m as Method>::NAME { return run::<$m>(core, input, ctx); } )*
            Err(ApiError::unknown_method(method))
        }

        /// Collects every API type, and the `Methods` map, as `(file name, contents)`.
        pub(crate) fn collect_bindings(
            cfg: &ts_rs::Config,
        ) -> std::collections::BTreeMap<String, String> {
            let mut out = bindings::Collector::new(cfg);
            out.add::<ApiError>();
            $( out.add::<<$m as Method>::Input>(); out.add::<<$m as Method>::Output>(); )*
            let entries = vec![$( bindings::methods_entry::<$m>(cfg) ),*];
            out.finish(entries)
        }
    };
}

methods! {
    examples::GetCoreInfo,
    examples::ExampleDivide,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(method: &str, input: Value) -> Result<Value, ApiError> {
        dispatch(&Core::new(), method, input, &OpContext::uncancellable())
    }

    #[test]
    fn get_core_info_returns_the_core_version() {
        let out = call("getCoreInfo", Value::Null).unwrap();
        assert_eq!(out, json!({ "coreVersion": fc_core::version() }));
    }

    #[test]
    fn example_divide_returns_the_quotient() {
        let out = call("exampleDivide", json!({ "dividend": 7, "divisor": 2 })).unwrap();
        assert_eq!(out, json!({ "quotient": 3.5 }));
    }

    #[test]
    fn dividing_by_zero_is_a_readable_invalid_input_error() {
        let err = call("exampleDivide", json!({ "dividend": 1, "divisor": 0 })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert_eq!(
            err.message,
            "Can't divide by zero. Enter a divisor other than 0."
        );
    }

    #[test]
    fn unknown_method_is_reported() {
        let err = call("nope", Value::Null).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnknownMethod);
        assert!(err.message.contains("nope"));
    }

    #[test]
    fn malformed_input_is_a_readable_invalid_input_error() {
        let err = call("exampleDivide", json!({ "dividend": "x" })).unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(err.message.contains("exampleDivide"));
    }

    #[test]
    fn error_json_has_kind_and_message_only() {
        let err = ApiError::new(ErrorKind::NotFound, "Gone.");
        assert_eq!(
            serde_json::to_value(err).unwrap(),
            json!({ "kind": "notFound", "message": "Gone." })
        );
    }

    #[test]
    fn checkpoint_reports_cancellation() {
        assert!(OpContext::uncancellable().checkpoint().is_ok());
        let err = OpContext::new(&|| true).checkpoint().unwrap_err();
        assert_eq!(err.kind, ErrorKind::Cancelled);
    }
}
