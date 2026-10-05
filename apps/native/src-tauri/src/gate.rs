//! Session token for the bridge commands (ADR 0005). On Android a sandboxed card frame can invoke
//! every command granted to the main window, so each command also needs a token that only the main
//! frame holds. The main frame claims it with `handshake` before any card frame exists.

use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use fc_api::{ApiError, ErrorKind};

const TOKEN_BYTES: usize = 16;

#[derive(Default)]
struct State {
    /// True once a token has been handed out, until `reset`.
    issued: bool,
    token: Option<String>,
}

#[derive(Default)]
pub struct Gate {
    state: Mutex<State>,
}

impl Gate {
    /// Returns a fresh random token, or `None` if one was already issued since the last `reset`.
    pub fn issue(&self) -> Option<String> {
        let mut state = self.state.lock().expect("gate lock");
        if state.issued {
            return None;
        }
        let mut bytes = [0u8; TOKEN_BYTES];
        // Failing to get randomness must never fall back to a guessable token: issue none.
        getrandom::fill(&mut bytes).ok()?;
        let token = URL_SAFE_NO_PAD.encode(bytes);
        state.issued = true;
        state.token = Some(token.clone());
        Some(token)
    }

    /// Forgets the token, so the next `issue` succeeds. Called when the main frame starts loading.
    pub fn reset(&self) {
        *self.state.lock().expect("gate lock") = State::default();
    }

    /// True only for the issued token. The comparison takes the same time wherever the strings
    /// differ.
    pub fn check(&self, candidate: &str) -> bool {
        let state = self.state.lock().expect("gate lock");
        match state.token.as_deref() {
            Some(token) => constant_time_eq(token.as_bytes(), candidate.as_bytes()),
            None => false,
        }
    }

    /// The check every bridge command calls first.
    pub fn authorize(&self, token: &str) -> Result<(), ApiError> {
        if self.check(token) {
            Ok(())
        } else {
            Err(connect_error())
        }
    }
}

/// What the UI sees when the handshake or a token check fails.
pub fn connect_error() -> ApiError {
    ApiError::new(
        ErrorKind::Internal,
        "The app could not connect to its core. Restart the app.",
    )
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    // The length is not secret (every token has the same length), so returning early is fine.
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_one_token_per_session() {
        let gate = Gate::default();
        assert!(gate.issue().is_some());
        assert!(gate.issue().is_none());
    }

    #[test]
    fn reset_allows_a_new_token_and_invalidates_the_old_one() {
        let gate = Gate::default();
        let old = gate.issue().unwrap();
        gate.reset();
        assert!(!gate.check(&old));
        let new = gate.issue().unwrap();
        assert_ne!(old, new);
        assert!(gate.check(&new));
    }

    #[test]
    fn accepts_only_the_issued_token() {
        let gate = Gate::default();
        assert!(!gate.check(""), "nothing issued yet");
        let token = gate.issue().unwrap();
        assert!(gate.check(&token));
        assert!(!gate.check(""));
        assert!(!gate.check("not-the-token"));
        assert!(!gate.check(&token[..token.len() - 1]));
        assert!(!gate.check(&format!("{token}x")));
    }

    #[test]
    fn authorize_rejects_missing_and_wrong_tokens_with_an_internal_error() {
        let gate = Gate::default();
        let before = gate.authorize("anything").unwrap_err();
        assert_eq!(before.kind, ErrorKind::Internal);
        assert_eq!(
            before.message,
            "The app could not connect to its core. Restart the app."
        );
        let token = gate.issue().unwrap();
        assert_eq!(gate.authorize("").unwrap_err().kind, ErrorKind::Internal);
        assert_eq!(
            gate.authorize("guess").unwrap_err().kind,
            ErrorKind::Internal
        );
        assert!(gate.authorize(&token).is_ok());
    }

    #[test]
    fn tokens_are_128_bits_of_base64() {
        let gate = Gate::default();
        let token = gate.issue().unwrap();
        assert_eq!(token.len(), 22);
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
    }
}
