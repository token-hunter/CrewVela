//! Platform-neutral decisions for a spawn-owned, still-waitable child.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LeaderGroupState { Known, ExitedWithoutLookup }

/// None represents ESRCH only; other system errors must stay errors at the caller.
pub(super) fn validate_leader_group(
    expected: u32,
    observed: Option<u32>,
    exited_unreaped: bool,
    darwin: bool,
) -> Result<LeaderGroupState, &'static str> {
    if observed == Some(expected) { return Ok(LeaderGroupState::Known); }
    if darwin && exited_unreaped && observed.is_none() {
        return Ok(LeaderGroupState::ExitedWithoutLookup);
    }
    Err("原生进程组身份发生变化")
}

/// An exited Darwin leader is not permission to signal an unverified group.
/// It can be finalized only after both executable-member checks succeeded.
pub(super) fn verify_exited_group_quiescent(active_members: bool, other_session_groups: bool) -> Result<(), &'static str> {
    if active_members || other_session_groups {
        Err("已退出的原生组首领仍有关联活动成员，停止未验证；保留身份待重试")
    } else { Ok(()) }
}

/// A missing Darwin leader lookup may use only the session ownership observed
/// while registering that leader and a still-live member of its original group.
/// Neither an unknown session nor a member in another group/session grants
/// permission to send a group signal.
pub(super) fn verify_owned_session_group_member(
    expected: u32,
    session_confirmed: bool,
    group: Option<u32>,
    session: Option<u32>,
) -> Result<(), &'static str> {
    if session_confirmed && group == Some(expected) && session == Some(expected) {
        Ok(())
    } else {
        Err("无法核实已退出首领的原始会话和进程组成员，保留身份")
    }
}

/// A failed signal can coincide with completed termination, but neither an
/// unowned/reaped leader nor an unavailable/live snapshot proves completion.
pub(super) fn signal_race_is_quiescent(exited_unreaped: bool, active_members: Option<bool>) -> bool {
    exited_unreaped && active_members == Some(false)
}

/// Darwin's E flag means P_WEXIT, not a completed/reapable exit. A complete
/// snapshot containing only exiting members (and zombies) permits waiting
/// within the existing stop deadline, never finalizing the identity.
pub(super) fn signal_race_can_wait(owned_child: bool, states: Option<&[&str]>) -> bool {
    fn state_flags(state: &str) -> Option<(bool, bool)> {
        let mut chars = state.chars();
        let primary = chars.next()?;
        if !matches!(primary, '?' | 'I' | 'R' | 'S' | 'T' | 'U' | 'Z') { return None; }
        let mut exiting = false;
        for flag in chars {
            if !matches!(flag, '<' | 'N' | 'X' | 'E' | 'V' | 'L' | 's' | '+') { return None; }
            exiting |= flag == 'E';
        }
        Some((primary == 'Z', exiting))
    }
    owned_child && states.is_some_and(|states| {
        !states.is_empty()
            && states.iter().any(|state| state_flags(state).is_some_and(|(_, exiting)| exiting))
            && states.iter().all(|state| state_flags(state).is_some_and(|(zombie, exiting)| zombie || exiting))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn darwin_unreaped_exited_child_may_be_missing_from_getpgid() {
        assert_eq!(validate_leader_group(42, None, true, true), Ok(LeaderGroupState::ExitedWithoutLookup));
    }
    #[test]
    fn exited_missing_group_never_finishes_with_live_members() {
        assert!(verify_exited_group_quiescent(true, false).is_err());
        assert!(verify_exited_group_quiescent(false, true).is_err());
        assert!(verify_exited_group_quiescent(true, true).is_err());
        assert_eq!(verify_exited_group_quiescent(false, false), Ok(()));
    }
    #[test]
    fn live_missing_child_and_changed_groups_are_never_accepted() {
        assert!(validate_leader_group(42, None, false, true).is_err());
        assert!(validate_leader_group(42, None, true, false).is_err());
        assert!(validate_leader_group(42, Some(7), true, true).is_err());
        assert!(validate_leader_group(42, Some(7), false, true).is_err());
        assert_eq!(validate_leader_group(42, Some(42), false, true), Ok(LeaderGroupState::Known));
        assert_eq!(validate_leader_group(42, Some(42), true, false), Ok(LeaderGroupState::Known));
    }
    #[test]
    fn exited_session_group_requires_registered_ownership_and_matching_live_member() {
        assert!(verify_owned_session_group_member(42, true, Some(42), Some(42)).is_ok());
        for (confirmed, group, session) in [
            (false, Some(42), Some(42)),
            (true, None, Some(42)),
            (true, Some(42), None),
            (true, Some(7), Some(42)),
            (true, Some(42), Some(7)),
        ] {
            assert!(verify_owned_session_group_member(42, confirmed, group, session).is_err());
        }
    }
    #[test]
    fn failed_signal_requires_owned_exit_and_complete_quiescent_snapshot() {
        assert!(signal_race_is_quiescent(true, Some(false)));
        assert!(!signal_race_is_quiescent(true, Some(true)), "EPERM with a live member must fail");
        assert!(!signal_race_is_quiescent(true, None), "query failure is not proof of exit");
        assert!(!signal_race_is_quiescent(false, Some(false)), "an unowned or live leader is not a reserved identity");
    }
    #[test]
    fn darwin_exiting_snapshot_only_defers_complete_owned_groups() {
        assert!(signal_race_can_wait(true, Some(&["?Es", "?E"])));
        assert!(signal_race_can_wait(true, Some(&["?E", "Z+"])));
        assert!(!signal_race_can_wait(false, Some(&["?E"])));
        assert!(!signal_race_can_wait(true, None));
        assert!(!signal_race_can_wait(true, Some(&[])));
        for state in ["S", "R+", "T", "U", "?", "", "QE", "?Eunknown", "Zunknown"] {
            assert!(!signal_race_can_wait(true, Some(&["?E", state])), "{state}");
        }
        assert!(!signal_race_can_wait(true, Some(&["Z", "Z+"])), "zombies are completion evidence only after owned waitid exit");
    }
}
