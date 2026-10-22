//! A Practice driver with test knobs, for the question tests (`questions.rs`, `question_delivery.rs`).
//! Not part of `common/mod.rs`: each test file that uses it says `#[path = "common/knobbed.rs"] mod knobbed;`.
#![allow(dead_code)]
use crate::common::m2::Served;
use model::{Answer, Caps, PermissionPosture};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// What one answer call does once it reaches the agent's program.
#[derive(Clone, Copy, Debug, Default)]
pub enum AnswerPlan {
    /// Passes it on at once.
    #[default]
    Pass,
    /// Waits, then passes it on.
    Delay(Duration),
    /// Waits, then reports success without passing it on: a delivery lost on the way.
    Drop(Duration),
    /// Waits, then refuses it with `REFUSAL`.
    Refuse(Duration),
}

/// The words a refused answer carries (inside the driver's `Unsupported` text).
pub const REFUSAL: &str = "test: this answer is refused";

/// A Practice driver (named "stub", so Practice agents use it). `restricted` makes every agent restricted
/// (Task 4 makes posture a setting; until then this is the only way to get one). The n-th answer call
/// follows `plan[n]`, and calls past the plan follow `then`. `calls` counts every answer call.
#[derive(Clone, Default)]
pub struct Knobbed {
    pub restricted: bool,
    pub plan: Vec<AnswerPlan>,
    pub then: AnswerPlan,
    pub calls: Arc<AtomicUsize>,
}

struct KnobbedSession {
    inner: Box<dyn drivers::Session>,
    knobs: Knobbed,
}

#[async_trait::async_trait]
impl drivers::Driver for Knobbed {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        drivers::stub::StubDriver.capabilities()
    }
    async fn launch(
        &self,
        mut spec: drivers::LaunchSpec,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        if self.restricted {
            spec.posture = PermissionPosture::Restricted;
        }
        let inner = drivers::stub::StubDriver.launch(spec).await?;
        Ok(Box::new(KnobbedSession {
            inner,
            knobs: self.clone(),
        }))
    }
    async fn resume(
        &self,
        spec: drivers::LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        let inner = drivers::stub::StubDriver.resume(spec, native_id).await?;
        Ok(Box::new(KnobbedSession {
            inner,
            knobs: self.clone(),
        }))
    }
}

#[async_trait::async_trait]
impl drivers::Session for KnobbedSession {
    async fn send(&mut self, msg: drivers::OutboundMessage) -> Result<(), drivers::DriverError> {
        self.inner.send(msg).await
    }
    async fn interrupt(&mut self) -> Result<(), drivers::DriverError> {
        self.inner.interrupt().await
    }
    async fn stop(&mut self, grace: Duration) -> Result<(), drivers::DriverError> {
        self.inner.stop(grace).await
    }
    async fn answer(
        &mut self,
        question_id: &str,
        answer: Answer,
    ) -> Result<(), drivers::DriverError> {
        let n = self.knobs.calls.fetch_add(1, Ordering::SeqCst);
        match self.knobs.plan.get(n).copied().unwrap_or(self.knobs.then) {
            AnswerPlan::Pass => self.inner.answer(question_id, answer).await,
            AnswerPlan::Delay(d) => {
                tokio::time::sleep(d).await;
                self.inner.answer(question_id, answer).await
            }
            AnswerPlan::Drop(d) => {
                tokio::time::sleep(d).await;
                Ok(())
            }
            AnswerPlan::Refuse(d) => {
                tokio::time::sleep(d).await;
                Err(drivers::DriverError::Unsupported(REFUSAL))
            }
        }
    }
    fn take_events(&mut self) -> Option<futures::stream::BoxStream<'static, drivers::DriverEvent>> {
        self.inner.take_events()
    }
}

/// A served controld with a `Knobbed` host and a started Practice "solo" team; returns its agent.
pub async fn ready_with(knobs: Knobbed) -> (Served, String) {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect(&s, vec![Arc::new(knobs)], None).await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    (s, agent)
}
