use tonic::{Request, Response, Status};

use crate::control_plane::ControlPlane;
use crate::grpc::auth::authorize_mobile_api_request;
use crate::grpc::events::{MobileEventStream, mobile_events};
use crate::grpc::proto;
use crate::grpc::proto::looper_realtime_server::LooperRealtime;
use crate::mobile_api::mobile_session_detail;
use crate::mobile_prompt_delivery::{PromptDispatch, mobile_desktop_snapshot, send_session_prompt};
use crate::mobile_session::{ASSISTANT_SURFACES, MobileSessionError};

const HEALTH_SERVICE_NAME: &str = "looper-realtime";
const DISPATCH_DELIVERED: &str = "delivered";
const DISPATCH_QUEUED: &str = "queued";
const DISPATCH_RESUMED: &str = "resumed";

#[derive(Clone)]
pub struct LooperRealtimeService {
    control_plane: ControlPlane,
}

impl LooperRealtimeService {
    pub fn new(control_plane: ControlPlane) -> Self {
        Self { control_plane }
    }
}

#[tonic::async_trait]
impl LooperRealtime for LooperRealtimeService {
    type SubscribeMobileEventsStream = MobileEventStream;
    type SubscribeDesktopEventsStream = MobileEventStream;

    async fn health(
        &self,
        _request: Request<proto::HealthRequest>,
    ) -> Result<Response<proto::HealthResponse>, Status> {
        let status = self.control_plane.status();
        Ok(Response::new(proto::HealthResponse {
            ok: status.source.health == "healthy",
            service: HEALTH_SERVICE_NAME.to_owned(),
            server_time: crate::mobile_events::mobile_event_now(),
        }))
    }

    async fn send_session_prompt(
        &self,
        request: Request<proto::SendSessionPromptRequest>,
    ) -> Result<Response<proto::SendSessionPromptResponse>, Status> {
        authorize_mobile_api_request(&self.control_plane, request.metadata())?;
        let request = request.into_inner();
        let assistant_surface = normalized_assistant_surface(&request.assistant_surface)?;
        ensure_mobile_session_visible(&self.control_plane, &request.thread_id, assistant_surface)?;
        let dispatch = send_session_prompt(
            &self.control_plane,
            &request.thread_id,
            assistant_surface,
            &request.prompt,
        )
        .map_err(mobile_session_status)?;

        Ok(Response::new(prompt_response(dispatch)))
    }

    async fn subscribe_mobile_events(
        &self,
        request: Request<proto::SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeMobileEventsStream>, Status> {
        authorize_mobile_api_request(&self.control_plane, request.metadata())?;
        Ok(Response::new(mobile_events(self.control_plane.clone())))
    }

    async fn subscribe_desktop_events(
        &self,
        _request: Request<proto::SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeDesktopEventsStream>, Status> {
        Ok(Response::new(mobile_events(self.control_plane.clone())))
    }
}

fn normalized_assistant_surface(value: &str) -> Result<Option<&str>, Status> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if ASSISTANT_SURFACES.contains(&value) {
        return Ok(Some(value));
    }
    Err(Status::invalid_argument("invalid assistant surface"))
}

fn ensure_mobile_session_visible(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<(), Status> {
    let snapshot = mobile_desktop_snapshot(control_plane)
        .map_err(|error| Status::internal(error.to_string()))?;
    let session_state = control_plane
        .mobile_session_service()
        .state()
        .map_err(mobile_session_status)?;
    if mobile_session_detail(&snapshot, &session_state, thread_id, assistant_surface).is_some() {
        return Ok(());
    }
    Err(Status::not_found("session not found"))
}

fn prompt_response(dispatch: PromptDispatch) -> proto::SendSessionPromptResponse {
    match dispatch {
        PromptDispatch::Delivered { prompt_id } => proto::SendSessionPromptResponse {
            accepted: true,
            dispatch_kind: DISPATCH_DELIVERED.to_owned(),
            prompt_id,
        },
        PromptDispatch::Queued { prompt_id } => proto::SendSessionPromptResponse {
            accepted: true,
            dispatch_kind: DISPATCH_QUEUED.to_owned(),
            prompt_id,
        },
        PromptDispatch::Resumed => proto::SendSessionPromptResponse {
            accepted: true,
            dispatch_kind: DISPATCH_RESUMED.to_owned(),
            prompt_id: String::new(),
        },
    }
}

fn mobile_session_status(error: MobileSessionError) -> Status {
    match error {
        MobileSessionError::SessionNotFound => Status::not_found(error.to_string()),
        MobileSessionError::PromptRequired
        | MobileSessionError::InvalidPreset
        | MobileSessionError::InvalidScope
        | MobileSessionError::InvalidAssistantSurface
        | MobileSessionError::InvalidNotificationChannel
        | MobileSessionError::MissingNotificationConfig
        | MobileSessionError::InvalidCompletionCheck => Status::invalid_argument(error.to_string()),
        MobileSessionError::ModeRequired => Status::failed_precondition(error.to_string()),
        MobileSessionError::SessionArchived
        | MobileSessionError::PromptDeliveryUnavailable
        | MobileSessionError::PromptDeliveryUnavailableReason(_)
        | MobileSessionError::PromptResumeUnavailable(_)
        | MobileSessionError::PromptSnapshotUnavailable(_) => {
            Status::failed_precondition(error.to_string())
        }
        MobileSessionError::NotificationNotFound | MobileSessionError::CompletionCheckNotFound => {
            Status::not_found(error.to_string())
        }
        MobileSessionError::Store(_)
        | MobileSessionError::Filesystem(_)
        | MobileSessionError::TimeFormat(_) => Status::internal(error.to_string()),
    }
}
