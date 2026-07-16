//! Formalizes the intended `InstanceBridgeOutStatus` transition graph for bridge-out
//! `Instance`s (the swap/escrow-based bridge-out path), and (under `#[cfg(kani)]`) proves
//! properties about it with Kani. Companion to `instance_bridge_in_transition.rs`, which covers the
//! bridge-in (`InstanceBridgeInStatus`) side of the same `Instance` type.
//!
//! As with bridge-in, `bitvm3_instance.ivy` does not constrain legal `bridge_out_status`
//! transitions -- its `set_bridge_out_status` action only requires `instance_exists` and the
//! kind matching, with no allowed-transition axiom (compare `bitvm3_graph.ivy`'s
//! `allowed_graph_transition`). The graph below is a reconstruction of the transition
//! relation actually driven by the event-watch/maintenance tasks, not an existing spec.
//!
//! State diagram (matches `allowed_bridge_out_transition` exactly):
//!
//! ```text
//! Initialize
//!     ├──────────────► Claim
//!     │
//!     ▼
//! Timeout
//!     │
//!     ▼
//! Refund
//! ```
//!
//! Edge citations:
//! - (created as) `Initialize`: node/src/scheduled_tasks/event_watch_task.rs:634-658
//!   (`handle_swap_init_events`, on a `SwapInitializeEvent`) and
//!   node/src/rpc_service/handler/bitvm2_handler.rs:343-354 (the `bridge-out init-tag` RPC
//!   handler, an earlier step of the same escrow flow that can create the row first). Both
//!   creation sites start the instance at `Initialize`.
//! - `Initialize -> Timeout`: node/src/scheduled_tasks/instance_maintenance_tasks.rs:482-517
//!   (`instance_bridge_out_monitor`), only considered for instances with `escrow_hash` set
//!   (line 491) and past their `bridge_out_lock_time` deadline (lines 498-508).
//! - `Initialize -> Claim`: node/src/scheduled_tasks/event_watch_task.rs:718-799
//!   (`handle_swap_claim_events`, on a `SwapClaimEvent`). The write itself
//!   (event_watch_task.rs:756-763) is unconditional -- it does not check the instance's
//!   current status before overwriting it, so this is the *intended* edge, not an enforced one.
//! - `Timeout -> Refund`: node/src/scheduled_tasks/event_watch_task.rs:801-834
//!   (`handle_swap_refund_events`, on a `SwapRefundEvent`). Also unconditional
//!   (event_watch_task.rs:810-815).
//!
//! `Claim` and `Refund` are alternate terminal outcomes of the same escrow: the user either
//! claims the swap or gets refunded after timeout, never both.

use store::InstanceBridgeOutStatus;

/// All 4 `InstanceBridgeOutStatus` variants (unlike `InstanceBridgeInStatus`, none of these
/// are display-only projections -- every variant is written to `Instance.status`).
pub const ALL_STATES: [InstanceBridgeOutStatus; 4] = [
    InstanceBridgeOutStatus::Initialize,
    InstanceBridgeOutStatus::Claim,
    InstanceBridgeOutStatus::Timeout,
    InstanceBridgeOutStatus::Refund,
];

/// Intended transition graph for `Instance.status` while `!is_bridge_in`. See the module doc
/// comment for per-edge citations.
pub fn allowed_bridge_out_transition(
    from: InstanceBridgeOutStatus,
    to: InstanceBridgeOutStatus,
) -> bool {
    use InstanceBridgeOutStatus::*;
    matches!((from, to), (Initialize, Claim) | (Initialize, Timeout) | (Timeout, Refund))
}

/// States with no outgoing edge in the intended graph.
pub fn is_terminal(status: InstanceBridgeOutStatus) -> bool {
    use InstanceBridgeOutStatus::*;
    matches!(status, Claim | Refund)
}

/// Mirrors the *actual* production mutation path today: all three bridge-out writers
/// (`handle_swap_claim_events`, `handle_swap_refund_events`, and the Timeout branch of
/// `instance_bridge_out_monitor`) issue an unconditional `InstanceUpdate::with_status(...)` --
/// none of them checks the row's current status before overwriting it.
pub fn unguarded_set_status(
    _from: InstanceBridgeOutStatus,
    to: InstanceBridgeOutStatus,
) -> InstanceBridgeOutStatus {
    to
}

/// A guarded setter that would close the gap: reject any transition not present in
/// `allowed_bridge_out_transition`. Proposed remediation, not (yet) wired into any call site.
pub fn try_transition_bridge_out_status(
    from: InstanceBridgeOutStatus,
    to: InstanceBridgeOutStatus,
) -> Result<InstanceBridgeOutStatus, (InstanceBridgeOutStatus, InstanceBridgeOutStatus)> {
    if allowed_bridge_out_transition(from.clone(), to.clone()) {
        Ok(to)
    } else {
        Err((from, to))
    }
}

#[cfg(kani)]
mod verification {
    use super::*;

    /// Kani enumerates a bounded index and maps it onto the 4 states so symbolic execution
    /// only ever sees valid enum values (avoids needing a `kani::Arbitrary` impl / derive on
    /// `InstanceBridgeOutStatus`).
    fn any_status() -> InstanceBridgeOutStatus {
        let idx: usize = kani::any();
        kani::assume(idx < ALL_STATES.len());
        ALL_STATES[idx].clone()
    }

    /// `Claim` and `Refund` are mutually exclusive terminal outcomes: neither has any further
    /// outgoing edge, so an instance can never flip from one into the other.
    #[kani::proof]
    fn claim_and_refund_are_absorbing() {
        let from = any_status();
        let to = any_status();
        if is_terminal(from.clone()) {
            assert!(!allowed_bridge_out_transition(from, to));
        }
    }

    /// `Refund` is only reachable through `Timeout` -- a user can't be refunded without the
    /// escrow deadline having passed first.
    #[kani::proof]
    fn refund_requires_prior_timeout() {
        let from = any_status();
        let to = any_status();
        if allowed_bridge_out_transition(from.clone(), to.clone())
            && to == InstanceBridgeOutStatus::Refund
        {
            assert_eq!(from, InstanceBridgeOutStatus::Timeout);
        }
    }

    /// The guarded setter can never produce a transition outside the intended graph.
    #[kani::proof]
    fn guarded_setter_never_violates_spec() {
        let from = any_status();
        let to = any_status();
        if let Ok(result) = try_transition_bridge_out_status(from.clone(), to) {
            assert!(allowed_bridge_out_transition(from, result));
        }
    }

    /// EXPECTED TO FAIL. This is the actual finding: it shows the real production writers can
    /// realize a transition the intended graph forbids -- e.g. a stray/replayed
    /// `SwapClaimEvent` landing on an instance that's already `Refund`, silently moving it to
    /// `Claim` (or vice versa with a `SwapRefundEvent` after a `Claim`), since none of the
    /// three writers check the row's current status first. `cargo kani` should report a
    /// concrete counterexample (from, to) pair for this harness.
    #[kani::proof]
    fn unguarded_setter_can_violate_spec() {
        let from = any_status();
        let to = any_status();
        let result = unguarded_set_status(from.clone(), to);
        assert!(allowed_bridge_out_transition(from, result));
    }
}
