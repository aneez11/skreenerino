use skreen_proto::{DeviceId, Role, RoomId, StreamId};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParticipantState {
    Idle,
    Presenting(StreamId),
    Viewing(StreamId),
}

#[derive(Debug, Clone)]
pub struct Participant {
    pub device: DeviceId,
    pub role: Role,
    pub state: ParticipantState,
}

#[derive(Debug, Default)]
pub struct Room {
    pub id: Option<RoomId>,
    pub coordinator: Option<DeviceId>,
    pub participants: HashMap<String, Participant>,
    pub streams: HashMap<String, DeviceId>,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("unknown participant: {0}")]
    UnknownParticipant(String),
    #[error("permission denied")]
    PermissionDenied,
    #[error("stream not found: {0}")]
    StreamNotFound(String),
}

impl Room {
    pub fn join(&mut self, device: DeviceId, role: Role) {
        let is_first = self.participants.is_empty();
        let id = device.0.clone();
        self.participants.insert(id.clone(), Participant { device: device.clone(), role, state: ParticipantState::Idle });
        if is_first { self.coordinator = Some(device); }
    }

    pub fn leave(&mut self, device: &DeviceId) -> Option<Participant> {
        let removed = self.participants.remove(&device.0);
        self.streams.retain(|_, owner| owner != device);
        if self.coordinator.as_ref() == Some(device) {
            self.coordinator = self.participants.keys().next().map(|k| DeviceId(k.clone()));
        }
        removed
    }

    pub fn request_present(&mut self, device: &DeviceId, stream: StreamId) -> Result<(), SessionError> {
        let p = self.participants.get_mut(&device.0).ok_or_else(|| SessionError::UnknownParticipant(device.0.clone()))?;
        p.state = ParticipantState::Presenting(stream.clone());
        self.streams.insert(stream.0, device.clone());
        Ok(())
    }

    pub fn request_view(&mut self, device: &DeviceId, stream: &StreamId) -> Result<(), SessionError> {
        if !self.streams.contains_key(&stream.0) { return Err(SessionError::StreamNotFound(stream.0.clone())); }
        let p = self.participants.get_mut(&device.0).ok_or_else(|| SessionError::UnknownParticipant(device.0.clone()))?;
        p.state = ParticipantState::Viewing(stream.clone());
        Ok(())
    }

    pub fn transfer_presenter(&mut self, from: &DeviceId, to: &DeviceId, stream: &StreamId) -> Result<(), SessionError> {
        if self.streams.get(&stream.0) != Some(from) { return Err(SessionError::PermissionDenied); }
        let _ = self.participants.get(&to.0).ok_or_else(|| SessionError::UnknownParticipant(to.0.clone()))?;
        self.streams.insert(stream.0.clone(), to.clone());
        if let Some(p) = self.participants.get_mut(&from.0) { p.state = ParticipantState::Idle; }
        if let Some(p) = self.participants.get_mut(&to.0) { p.state = ParticipantState::Presenting(stream.clone()); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_leave_coordinator_handover() {
        let mut room = Room::default();
        let a = DeviceId("a".into()); let b = DeviceId("b".into());
        room.join(a.clone(), Role::Both);
        room.join(b.clone(), Role::Viewer);
        assert_eq!(room.coordinator.as_ref(), Some(&a));
        room.leave(&a);
        assert_eq!(room.coordinator.as_ref(), Some(&b));
    }

    #[test]
    fn present_view_transfer() {
        let mut room = Room::default();
        let a = DeviceId("a".into()); let b = DeviceId("b".into());
        room.join(a.clone(), Role::Both); room.join(b.clone(), Role::Both);
        let s = StreamId("s1".into());
        room.request_present(&a, s.clone()).unwrap();
        room.request_view(&b, &s).unwrap();
        room.transfer_presenter(&a, &b, &s).unwrap();
        assert_eq!(room.streams.get("s1"), Some(&b));
    }
}
