use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::ClientCoreError;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientModeMutation {
    pub session_id: String,
    pub preset: String,
    pub client_mutation_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientModeMutationOption {
    pub has_mutation: bool,
    pub mutation: ClientModeMutation,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientModeMutationEnqueueResult {
    pub mutation: ClientModeMutation,
    pub should_start_drain: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientModeMutationDrainFinish {
    pub is_stale: bool,
    pub should_clear_rollback: bool,
    pub has_next_mutation: bool,
    pub next_mutation: ClientModeMutation,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientModeMutationBatchFinish {
    pub was_latest: bool,
    pub should_cancel_active_drain: bool,
    pub should_clear_rollback: bool,
    pub has_next_mutation: bool,
    pub next_mutation: ClientModeMutation,
}

#[derive(Debug, Default)]
struct SessionModeMutationState {
    active_drain_id: Option<String>,
    pending: Vec<ClientModeMutation>,
    latest: Option<ClientModeMutation>,
}

#[derive(Debug, Default)]
struct ModeMutationQueueState {
    sessions: BTreeMap<String, SessionModeMutationState>,
}

#[derive(Debug, uniffi::Object)]
pub struct ClientModeMutationQueue {
    state: Mutex<ModeMutationQueueState>,
}

#[uniffi::export]
impl ClientModeMutationQueue {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ModeMutationQueueState::default()),
        })
    }

    pub fn enqueue_mode_mutation(
        &self,
        session_id: String,
        preset: String,
        client_mutation_id: String,
    ) -> Result<ClientModeMutationEnqueueResult, ClientCoreError> {
        require_present(&session_id, ClientCoreError::EmptySessionId)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mutation = ClientModeMutation {
            session_id: session_id.clone(),
            preset,
            client_mutation_id,
        };
        let mut state = self.lock_state()?;
        let session = state.sessions.entry(session_id).or_default();
        session.latest = Some(mutation.clone());

        let should_start_drain = session.active_drain_id.is_none();
        if !should_start_drain {
            session.pending.push(mutation.clone());
        }

        Ok(ClientModeMutationEnqueueResult {
            mutation,
            should_start_drain,
        })
    }

    pub fn start_mode_drain(
        &self,
        session_id: String,
        drain_id: String,
    ) -> Result<(), ClientCoreError> {
        require_present(&session_id, ClientCoreError::EmptySessionId)?;
        require_present(&drain_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        let session = state.sessions.entry(session_id).or_default();
        session.active_drain_id = Some(drain_id);
        Ok(())
    }

    pub fn take_next_mode_mutation(
        &self,
        session_id: String,
    ) -> Result<ClientModeMutationOption, ClientCoreError> {
        require_present(&session_id, ClientCoreError::EmptySessionId)?;

        let mut state = self.lock_state()?;
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return Ok(no_mode_mutation());
        };
        if session.pending.is_empty() {
            return Ok(no_mode_mutation());
        }

        Ok(some_mode_mutation(session.pending.remove(0)))
    }

    pub fn finish_mode_drain(
        &self,
        session_id: String,
        drain_id: String,
    ) -> Result<ClientModeMutationDrainFinish, ClientCoreError> {
        require_present(&session_id, ClientCoreError::EmptySessionId)?;
        require_present(&drain_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return Ok(ClientModeMutationDrainFinish {
                is_stale: true,
                should_clear_rollback: false,
                has_next_mutation: false,
                next_mutation: empty_mode_mutation(),
            });
        };
        if session.active_drain_id.as_deref() != Some(drain_id.as_str()) {
            return Ok(ClientModeMutationDrainFinish {
                is_stale: true,
                should_clear_rollback: false,
                has_next_mutation: false,
                next_mutation: empty_mode_mutation(),
            });
        }

        session.active_drain_id = None;
        if !session.pending.is_empty() {
            let next_mutation = session.pending.remove(0);
            return Ok(ClientModeMutationDrainFinish {
                is_stale: false,
                should_clear_rollback: false,
                has_next_mutation: true,
                next_mutation,
            });
        }

        session.latest = None;
        Ok(ClientModeMutationDrainFinish {
            is_stale: false,
            should_clear_rollback: true,
            has_next_mutation: false,
            next_mutation: empty_mode_mutation(),
        })
    }

    pub fn finish_batched_mode_mutation(
        &self,
        session_id: String,
        client_mutation_id: String,
    ) -> Result<ClientModeMutationBatchFinish, ClientCoreError> {
        require_present(&session_id, ClientCoreError::EmptySessionId)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return Ok(ClientModeMutationBatchFinish {
                was_latest: false,
                should_cancel_active_drain: false,
                should_clear_rollback: false,
                has_next_mutation: false,
                next_mutation: empty_mode_mutation(),
            });
        };

        session
            .pending
            .retain(|mutation| mutation.client_mutation_id != client_mutation_id);
        let was_latest = session
            .latest
            .as_ref()
            .is_some_and(|mutation| mutation.client_mutation_id == client_mutation_id);
        if was_latest {
            session.latest = None;
        }

        let should_cancel_active_drain = session.active_drain_id.take().is_some();
        if !session.pending.is_empty() {
            let next_mutation = session.pending.remove(0);
            return Ok(ClientModeMutationBatchFinish {
                was_latest,
                should_cancel_active_drain,
                should_clear_rollback: false,
                has_next_mutation: true,
                next_mutation,
            });
        }

        Ok(ClientModeMutationBatchFinish {
            was_latest,
            should_cancel_active_drain,
            should_clear_rollback: was_latest,
            has_next_mutation: false,
            next_mutation: empty_mode_mutation(),
        })
    }

    pub fn latest_mode_mutation(
        &self,
        session_id: String,
    ) -> Result<ClientModeMutationOption, ClientCoreError> {
        require_present(&session_id, ClientCoreError::EmptySessionId)?;

        let state = self.lock_state()?;
        Ok(state
            .sessions
            .get(&session_id)
            .and_then(|session| session.latest.clone())
            .map(some_mode_mutation)
            .unwrap_or_else(no_mode_mutation))
    }

    pub fn is_latest_mode_mutation(
        &self,
        session_id: String,
        client_mutation_id: String,
    ) -> Result<bool, ClientCoreError> {
        require_present(&session_id, ClientCoreError::EmptySessionId)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let state = self.lock_state()?;
        Ok(state
            .sessions
            .get(&session_id)
            .and_then(|session| session.latest.as_ref())
            .is_some_and(|mutation| mutation.client_mutation_id == client_mutation_id))
    }

    pub fn clear(&self) -> Result<(), ClientCoreError> {
        let mut state = self.lock_state()?;
        state.sessions.clear();
        Ok(())
    }
}

impl ClientModeMutationQueue {
    fn lock_state(&self) -> Result<MutexGuard<'_, ModeMutationQueueState>, ClientCoreError> {
        self.state
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)
    }
}

fn some_mode_mutation(mutation: ClientModeMutation) -> ClientModeMutationOption {
    ClientModeMutationOption {
        has_mutation: true,
        mutation,
    }
}

fn no_mode_mutation() -> ClientModeMutationOption {
    ClientModeMutationOption {
        has_mutation: false,
        mutation: empty_mode_mutation(),
    }
}

fn empty_mode_mutation() -> ClientModeMutation {
    ClientModeMutation {
        session_id: String::new(),
        preset: String::new(),
        client_mutation_id: String::new(),
    }
}

fn require_present(value: &str, error: ClientCoreError) -> Result<(), ClientCoreError> {
    if value.trim().is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD_ID: &str = "thread-main";

    #[test]
    fn first_mode_mutation_starts_drain_and_next_mutation_queues() {
        let queue = ClientModeMutationQueue::new();
        let first = queue
            .enqueue_mode_mutation(
                THREAD_ID.to_owned(),
                "await-reply".to_owned(),
                "cmid-1".to_owned(),
            )
            .expect("first");
        queue
            .start_mode_drain(THREAD_ID.to_owned(), "drain-1".to_owned())
            .expect("start drain");
        let second = queue
            .enqueue_mode_mutation(
                THREAD_ID.to_owned(),
                "infinite".to_owned(),
                "cmid-2".to_owned(),
            )
            .expect("second");
        let latest = queue
            .latest_mode_mutation(THREAD_ID.to_owned())
            .expect("latest");

        assert!(first.should_start_drain);
        assert!(!second.should_start_drain);
        assert_eq!(latest.mutation.client_mutation_id, "cmid-2");
    }

    #[test]
    fn finish_drain_returns_queued_mutation_then_clears_rollback() {
        let queue = ClientModeMutationQueue::new();
        queue
            .enqueue_mode_mutation(
                THREAD_ID.to_owned(),
                "await-reply".to_owned(),
                "cmid-1".into(),
            )
            .expect("first");
        queue
            .start_mode_drain(THREAD_ID.to_owned(), "drain-1".to_owned())
            .expect("start drain");
        queue
            .enqueue_mode_mutation(THREAD_ID.to_owned(), "infinite".to_owned(), "cmid-2".into())
            .expect("second");

        let finish = queue
            .finish_mode_drain(THREAD_ID.to_owned(), "drain-1".to_owned())
            .expect("finish");
        queue
            .start_mode_drain(THREAD_ID.to_owned(), "drain-2".to_owned())
            .expect("second drain");
        let final_finish = queue
            .finish_mode_drain(THREAD_ID.to_owned(), "drain-2".to_owned())
            .expect("final finish");
        let latest = queue
            .latest_mode_mutation(THREAD_ID.to_owned())
            .expect("latest");

        assert!(!finish.is_stale);
        assert!(finish.has_next_mutation);
        assert_eq!(finish.next_mutation.client_mutation_id, "cmid-2");
        assert!(final_finish.should_clear_rollback);
        assert!(!latest.has_mutation);
    }

    #[test]
    fn stale_drain_finish_does_not_mutate_queue() {
        let queue = ClientModeMutationQueue::new();
        queue
            .enqueue_mode_mutation(
                THREAD_ID.to_owned(),
                "await-reply".to_owned(),
                "cmid-1".into(),
            )
            .expect("first");
        queue
            .start_mode_drain(THREAD_ID.to_owned(), "drain-1".to_owned())
            .expect("start drain");

        let finish = queue
            .finish_mode_drain(THREAD_ID.to_owned(), "old-drain".to_owned())
            .expect("finish");
        let latest = queue
            .latest_mode_mutation(THREAD_ID.to_owned())
            .expect("latest");

        assert!(finish.is_stale);
        assert_eq!(latest.mutation.client_mutation_id, "cmid-1");
    }

    #[test]
    fn batched_latest_mutation_cancels_active_drain_and_clears_rollback() {
        let queue = ClientModeMutationQueue::new();
        queue
            .enqueue_mode_mutation(
                THREAD_ID.to_owned(),
                "await-reply".to_owned(),
                "cmid-1".into(),
            )
            .expect("first");
        queue
            .start_mode_drain(THREAD_ID.to_owned(), "drain-1".to_owned())
            .expect("start drain");

        let finish = queue
            .finish_batched_mode_mutation(THREAD_ID.to_owned(), "cmid-1".to_owned())
            .expect("batch finish");
        let latest = queue
            .latest_mode_mutation(THREAD_ID.to_owned())
            .expect("latest");

        assert!(finish.was_latest);
        assert!(finish.should_cancel_active_drain);
        assert!(finish.should_clear_rollback);
        assert!(!latest.has_mutation);
    }

    #[test]
    fn clear_removes_all_mode_mutations() {
        let queue = ClientModeMutationQueue::new();
        queue
            .enqueue_mode_mutation(
                THREAD_ID.to_owned(),
                "await-reply".to_owned(),
                "cmid-1".into(),
            )
            .expect("first");
        queue.clear().expect("clear");
        let latest = queue
            .latest_mode_mutation(THREAD_ID.to_owned())
            .expect("latest");

        assert!(!latest.has_mutation);
    }
}
