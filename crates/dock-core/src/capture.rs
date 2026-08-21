//! When to remember a jump-back window. Presenter supplies snapshots;
//! this crate never calls Win32.

use crate::SessionState;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionKey {
    pub source: String,
    pub session_id: String,
}

impl SessionKey {
    pub fn new(source: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            session_id: session_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSession {
    pub source: String,
    pub session_id: String,
    pub state: SessionState,
    pub parent_session_id: Option<String>,
}

impl CaptureSession {
    pub fn new(
        source: impl Into<String>,
        session_id: impl Into<String>,
        state: SessionState,
    ) -> Self {
        Self {
            source: source.into(),
            session_id: session_id.into(),
            state,
            parent_session_id: None,
        }
    }

    pub fn with_parent(mut self, parent_session_id: impl Into<String>) -> Self {
        let parent = parent_session_id.into();
        self.parent_session_id = (!parent.trim().is_empty()).then_some(parent);
        self
    }

    pub fn key(&self) -> SessionKey {
        SessionKey::new(self.source.clone(), self.session_id.clone())
    }

    fn has_parent(&self) -> bool {
        self.parent_session_id
            .as_deref()
            .is_some_and(|parent| !parent.trim().is_empty())
    }
}

/// Sessions whose source terminal should be captured now.
///
/// New main sessions and transitions *into* `working` capture.
/// `working` → `working` and every other transition do not.
/// Child sessions (parent set) never capture, even if they appear or go working.
pub fn sessions_to_capture(
    previous: &[CaptureSession],
    current: &[CaptureSession],
) -> Vec<SessionKey> {
    let previous_by_key: HashMap<SessionKey, SessionState> = previous
        .iter()
        .map(|session| (session.key(), session.state))
        .collect();
    current
        .iter()
        .filter(|session| !session.has_parent())
        .filter_map(|session| match previous_by_key.get(&session.key()) {
            None => Some(session.key()),
            Some(previous_state)
                if session.state == SessionState::Working
                    && *previous_state != SessionState::Working =>
            {
                Some(session.key())
            }
            _ => None,
        })
        .collect()
}

/// Stored capture keys whose session is no longer in the snapshot.
pub fn captured_keys_to_drop(stored: &[SessionKey], live: &[SessionKey]) -> Vec<SessionKey> {
    stored
        .iter()
        .filter(|key| !live.contains(key))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{captured_keys_to_drop, sessions_to_capture, CaptureSession, SessionKey};
    use crate::SessionState;

    fn sess(id: &str, state: SessionState) -> CaptureSession {
        CaptureSession::new("grok", id, state)
    }

    #[test]
    fn new_session_is_captured() {
        let current = [sess("s1", SessionState::Idle)];
        assert_eq!(
            sessions_to_capture(&[], &current),
            [SessionKey::new("grok", "s1")]
        );
    }

    #[test]
    fn idle_to_working_is_captured() {
        let previous = [sess("s1", SessionState::Idle)];
        let current = [sess("s1", SessionState::Working)];
        assert_eq!(
            sessions_to_capture(&previous, &current),
            [SessionKey::new("grok", "s1")]
        );
    }

    #[test]
    fn completed_to_working_is_captured() {
        let previous = [sess("s1", SessionState::Completed)];
        let current = [sess("s1", SessionState::Working)];
        assert_eq!(
            sessions_to_capture(&previous, &current),
            [SessionKey::new("grok", "s1")]
        );
    }

    #[test]
    fn working_to_working_is_not_captured() {
        let previous = [sess("s1", SessionState::Working)];
        let current = [sess("s1", SessionState::Working)];
        assert!(sessions_to_capture(&previous, &current).is_empty());
    }

    #[test]
    fn parent_session_is_not_captured() {
        let current = [sess("child", SessionState::Working).with_parent("parent")];
        assert!(sessions_to_capture(&[], &current).is_empty());

        let previous = [sess("child", SessionState::Idle).with_parent("parent")];
        let current = [sess("child", SessionState::Working).with_parent("parent")];
        assert!(sessions_to_capture(&previous, &current).is_empty());
    }

    #[test]
    fn other_transitions_are_not_captured() {
        let previous = [sess("s1", SessionState::Working)];
        let current = [sess("s1", SessionState::Completed)];
        assert!(sessions_to_capture(&previous, &current).is_empty());
    }

    #[test]
    fn disappeared_session_keys_are_dropped() {
        let stored = [
            SessionKey::new("grok", "keep"),
            SessionKey::new("grok", "gone"),
        ];
        let live = [SessionKey::new("grok", "keep")];
        assert_eq!(
            captured_keys_to_drop(&stored, &live),
            [SessionKey::new("grok", "gone")]
        );
    }
}
