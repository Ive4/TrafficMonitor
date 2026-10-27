/// Completed milestones of the opt-in, synchronous hidden-window probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    ClassRegistered,
    ChildCreated,
    HiddenChildVerified,
    ChildDestroyed,
    ClassUnregistered,
}

#[cfg(any(windows, test))]
pub(crate) fn valid_transition(from: LifecycleState, to: LifecycleState) -> bool {
    use LifecycleState::*;
    matches!(
        (from, to),
        (ClassRegistered, ChildCreated | ClassUnregistered)
            | (ChildCreated, HiddenChildVerified | ChildDestroyed)
            | (HiddenChildVerified, ChildDestroyed)
            | (ChildDestroyed, ClassUnregistered)
    )
}

#[cfg(test)]
mod tests {
    use super::{LifecycleState::*, valid_transition};

    #[test]
    fn lifecycle_accepts_the_successful_ownership_sequence() {
        for (from, to) in [
            (ClassRegistered, ChildCreated),
            (ChildCreated, HiddenChildVerified),
            (HiddenChildVerified, ChildDestroyed),
            (ChildDestroyed, ClassUnregistered),
        ] {
            assert!(valid_transition(from, to));
        }
    }

    #[test]
    fn failed_creation_or_validation_can_still_release_resources() {
        assert!(valid_transition(ClassRegistered, ClassUnregistered));
        assert!(valid_transition(ChildCreated, ChildDestroyed));
    }

    #[test]
    fn destroyed_resources_cannot_be_verified_reused_or_closed_twice() {
        for (from, to) in [
            (ChildDestroyed, HiddenChildVerified),
            (ClassUnregistered, ChildCreated),
            (ClassUnregistered, ClassUnregistered),
            (ClassRegistered, HiddenChildVerified),
            (HiddenChildVerified, ClassUnregistered),
        ] {
            assert!(!valid_transition(from, to));
        }
    }
}
