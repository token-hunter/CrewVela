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
}
