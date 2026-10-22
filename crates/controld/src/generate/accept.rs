//! A drafted team the person has trimmed: the checks it would get, and the team made from it, with
//! where it came from (the prompt and the CLI that drafted it) kept as its provenance.
use crate::extract::Json;
use crate::portable::team::PortableTeam;
use crate::state::AppState;
use crate::views::{TeamOrigin, TeamRecord};
use axum::extract::State;
use axum::response::Response;
use model::{DraftIssue, TeamDraft};
use serde::{Deserialize, Serialize};

fn portable(draft: TeamDraft) -> PortableTeam {
    PortableTeam {
        team: draft,
        settings: Default::default(),
    }
}

#[derive(Deserialize)]
pub(super) struct CheckReq {
    draft: TeamDraft,
}

#[derive(Serialize)]
struct Checked {
    issues: Vec<DraftIssue>,
}

/// What Design would show on the team, without creating anything (the accept board's live issues).
pub(super) async fn check_draft(State(st): State<AppState>, Json(req): Json<CheckReq>) -> Response {
    crate::error::respond(
        crate::portable::team::check(&st, portable(req.draft))
            .map(|(_, issues)| Checked { issues }),
    )
}

#[derive(Deserialize, Serialize)]
pub(super) struct CreateReq {
    #[serde(default)]
    request_id: String,
    draft: TeamDraft,
    origin: TeamOrigin,
}

#[derive(Serialize)]
struct Created {
    team: TeamRecord,
    warnings: Vec<DraftIssue>,
}

pub(super) async fn create_draft(
    State(st): State<AppState>,
    Json(req): Json<CreateReq>,
) -> Response {
    let fp = crate::api::fingerprint("create-generated-team", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::api::idempotent(&st, &request_id, fp, move || async move {
        let (team, warnings) = crate::portable::team::check(&st2, portable(req.draft))?;
        let draft = crate::portable::team::create(&st2, team)?;
        st2.store
            .lock()
            .expect("store mutex must not be poisoned")
            .set_team_origin(&draft.id, &req.origin)?;
        crate::analytics::emit(
            &st2,
            "team.created",
            serde_json::json!({ "template": "generated" }),
        );
        crate::error::to_value(&Created {
            team: crate::teams::record(&st2, draft)?,
            warnings,
        })
    })
    .await
}
