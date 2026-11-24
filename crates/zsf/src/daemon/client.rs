//! The CLI side of the request queue: queue an action for the daemon and wait for its answer
//! (the daemon side is [`crate::engine::Engine`] reading [`Feature::pending_requests`]).

use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::zeroshot_for;
use crate::engine::Engine;
use crate::model::{Action, RequestResult};
use crate::store::Feature;

/// How often [`submit`] looks for the answer.
const ANSWER_POLL: Duration = Duration::from_millis(100);

/// Queues `action` and waits for its answer. When no daemon runs, this process becomes the
/// writer and applies the queue itself.
pub fn submit(feature: &Feature, action: Action, wait: Duration) -> Result<RequestResult> {
    let id = feature.queue_request(action)?;
    let deadline = Instant::now() + wait;
    loop {
        if let Some(result) = feature.request_result(&id) {
            return Ok(result);
        }
        // No daemon (or it just exited without seeing the request): apply the queue here.
        if let Some(_lock) = feature.try_writer_lock()? {
            if let Some(result) = feature.request_result(&id) {
                return Ok(result);
            }
            let mut engine = Engine::open(feature.clone(), zeroshot_for(feature))?;
            engine.apply_requests_now()?;
            return feature
                .request_result(&id)
                .context("the request was not answered");
        }
        if Instant::now() >= deadline {
            return Ok(RequestResult {
                id,
                ok: true,
                message: "queued; the daemon applies it when its current step is done".to_owned(),
            });
        }
        std::thread::sleep(ANSWER_POLL);
    }
}
