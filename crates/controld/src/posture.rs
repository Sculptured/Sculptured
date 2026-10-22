//! An agent's permission posture (M2 Task 4): its own setting decides, else its team's, else the
//! global setting (D7: unrestricted by default). Applies at the agent's next launch.
use crate::state::AppState;
use model::PermissionPosture;
use rigspec::CompiledSeat;

#[must_use]
pub fn for_seat(st: &AppState, rig: &str, seat: &CompiledSeat) -> PermissionPosture {
    if let Some(policy) = seat.permission_policy.as_deref() {
        return from_policy(policy);
    }
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let team = store.team_settings(rig).ok().and_then(|t| t.posture);
    team.or_else(|| store.settings().ok().map(|s| s.posture))
        .unwrap_or(PermissionPosture::Unrestricted)
}

/// `none`, `builtin:yolo` and `builtin:open` run unrestricted; any other policy is restricted.
fn from_policy(policy: &str) -> PermissionPosture {
    match policy {
        "none" | "builtin:yolo" | "builtin:open" => PermissionPosture::Unrestricted,
        _ => PermissionPosture::Restricted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_policies_run_unrestricted_and_every_other_policy_is_restricted() {
        for p in ["none", "builtin:yolo", "builtin:open"] {
            assert_eq!(from_policy(p), PermissionPosture::Unrestricted, "{p}");
        }
        for p in ["builtin:standard", "policies/team.yaml"] {
            assert_eq!(from_policy(p), PermissionPosture::Restricted, "{p}");
        }
    }
}
