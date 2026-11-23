//! Consent flow for push/pull screen sharing.

use crate::{Consent, ConsentKind, ConsentRequest};

#[derive(Debug, Default)]
pub struct ConsentGate {
    pending: Option<ConsentRequest>,
    allowed: Vec<String>,
}

impl ConsentGate {
    pub fn request(&mut self, requester: impl Into<String>, kind: ConsentKind) -> Consent {
        let requester = requester.into();
        if self.allowed.iter().any(|r| r == &requester) {
            return Consent::Accepted;
        }
        self.pending = Some(ConsentRequest { requester, kind });
        Consent::Expired
    }

    pub fn pending(&self) -> Option<&ConsentRequest> { self.pending.as_ref() }

    pub fn respond(&mut self, accepted: bool, remember: bool) -> Consent {
        let Some(req) = self.pending.take() else { return Consent::Expired };
        if accepted {
            if remember {
                self.allowed.push(req.requester.clone());
            }
            Consent::Accepted
        } else {
            Consent::Rejected
        }
    }

    pub fn revoke(&mut self, requester: &str) {
        self.allowed.retain(|r| r != requester);
    }

    pub fn allowlist(&self) -> &[String] { &self.allowed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_consent_then_remembers() {
        let mut gate = ConsentGate::default();
        assert_eq!(gate.request("bob", ConsentKind::Pull), Consent::Expired);
        assert!(gate.pending().is_some());
        assert_eq!(gate.respond(true, true), Consent::Accepted);
        assert_eq!(gate.request("bob", ConsentKind::Pull), Consent::Accepted);
    }

    #[test]
    fn rejection_is_not_remembered() {
        let mut gate = ConsentGate::default();
        gate.request("eve", ConsentKind::Push);
        assert_eq!(gate.respond(false, true), Consent::Rejected);
        assert_eq!(gate.request("eve", ConsentKind::Push), Consent::Expired);
        assert!(gate.allowlist().is_empty());
    }

    #[test]
    fn nothing_pending_returns_expired() {
        let mut gate = ConsentGate::default();
        assert_eq!(gate.respond(true, false), Consent::Expired);
    }
}

