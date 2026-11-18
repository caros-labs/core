use crate::supervisor::Supervisor;
use caros_protocol::{ApiError, ErrorCode, LifecycleEvent, Reply, Request};
use std::io;
use tokio::sync::{broadcast, oneshot};

pub(super) struct Command {
    pub request: Request,
    pub reply: oneshot::Sender<Answer>,
}
pub(super) struct Answer {
    pub result: Result<Reply, ApiError>,
    pub events: Option<broadcast::Receiver<LifecycleEvent>>,
}

pub(super) struct Dispatcher {
    pub supervisor: Supervisor,
    sequence: u64,
    events: broadcast::Sender<LifecycleEvent>,
}
impl Dispatcher {
    pub fn new(supervisor: Supervisor) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            supervisor,
            sequence: 0,
            events,
        }
    }
    pub fn publish(&mut self) {
        for event in self.supervisor.drain_events() {
            self.sequence += 1;
            let _ = self.events.send(LifecycleEvent {
                sequence: self.sequence,
                event: event.into(),
            });
        }
    }
    pub fn handle(&mut self, request: Request) -> Answer {
        let mut events = None;
        let result = match request {
            Request::ListApps => Ok(Reply::Apps(self.snapshot())),
            Request::AppStatus { id } => self
                .supervisor
                .status(&id)
                .map(|status| Reply::Status(status.into()))
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "app is not registered")),
            Request::StartApp { id } => self
                .supervisor
                .start(&id)
                .map(|()| Reply::Accepted)
                .map_err(map_error),
            Request::StopApp { id } => self
                .supervisor
                .stop(&id)
                .map(|()| Reply::Accepted)
                .map_err(map_error),
            Request::Subscribe => {
                // Subscribe and snapshot on the same owner, with no await in between.
                events = Some(self.events.subscribe());
                Ok(Reply::Subscribed {
                    apps: self.snapshot(),
                    sequence: self.sequence,
                })
            }
        };
        self.publish();
        Answer { result, events }
    }
    fn snapshot(&self) -> Vec<caros_protocol::AppStatus> {
        self.supervisor.list().into_iter().map(Into::into).collect()
    }
}
fn map_error(error: io::Error) -> ApiError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => ErrorCode::NotFound,
        io::ErrorKind::AlreadyExists => ErrorCode::AlreadyActive,
        io::ErrorKind::InvalidInput => ErrorCode::InvalidRequest,
        _ => ErrorCode::Internal,
    };
    ApiError::new(code, error.to_string())
}
