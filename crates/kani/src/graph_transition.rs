//! Formalizes the `GraphStatus` transition graph for BitVM2 `Graph`s, and (under
//! `#[cfg(kani)]`) proves properties about it with Kani. Companion to
//! `instance_bridge_in_transition.rs`/`instance_bridge_out_transition.rs`, which cover
//! `Instance.status` on the `Instance` each `Graph` belongs to (`graph_belongs_to` in
//! `bitvm3_graph.ivy`).
//!
//! Unlike the two Instance harness sets, this one is **not** a reconstruction:
//! `bitvm-node-formal-verification/spec/bitvm3_graph.ivy` already has a formal
//! `axiom allowed_graph_transition(F,T)` for `graph_status`. `allowed_graph_transition` below
//! is a direct transcription of that axiom (Ivy state name -> `store::GraphStatus` variant),
//! so this harness set checks conformance with an existing spec, not a hypothesis about one.
//!
//! Ivy state name -> `GraphStatus` variant: `operator_presigned` -> `OperatorPresigned`,
//! `committee_presigned` -> `CommitteePresigned`, `operator_data_pushed` ->
//! `OperatorDataPushed`, `pre_kickoff` -> `PreKickoff`, `operator_kickoff` ->
//! `OperatorKickOff`, `challenge` -> `Challenge`, `disprove` -> `Disprove`, `obsoleted` ->
//! `Obsoleted`, `skipped` -> `Skipped`, `operator_take1` -> `OperatorTake1`, `operator_take2`
//! -> `OperatorTake2`.
//!
//! State diagram (matches `allowed_graph_transition`'s non-reflexive edges; the axiom also
//! allows `F = T` for every state, i.e. every state self-loops, omitted below for
//! readability):
//!
//! ```text
//! OperatorPresigned
//!     ├──────────────► Obsoleted
//!     │
//!     ▼
//! CommitteePresigned
//!     ├──────────────► Obsoleted
//!     │
//!     ▼
//! OperatorDataPushed
//!     ├──────────────► Skipped
//!     ├──────────────► Obsoleted
//!     │
//!     ▼
//! PreKickoff
//!     ├──────────────► Skipped
//!     │
//!     ▼
//! OperatorKickOff
//!     ├──────────────► OperatorTake1
//!     ├──────────────► Disprove
//!     │
//!     ▼
//! Challenge
//!     ├──────────────► Disprove
//!     └──────────────► OperatorTake2
//! ```
//!
//! Not shown above (to keep the spine readable): `Obsoleted` independently reaches the same
//! two destinations as `PreKickoff` (`Obsoleted -> OperatorKickOff`, `Obsoleted -> Skipped`) --
//! it is not itself reached *from* `PreKickoff`. See `allowed_graph_transition` for the exact
//! edge set, or `bitvm3_graph.ivy`'s `axiom allowed_graph_transition` for the source.
//!
//! `OperatorTake1`, `OperatorTake2`, `Disprove`, and `Skipped` have no outgoing edge other than
//! the self-loop -- they are exactly `store::GraphStatus::get_closed_status()`. Because
//! `OperatorKickOff` and `Challenge` are the only states with more than one outgoing edge, and
//! all four closed states are unreachable from each other, this single-step transition
//! relation already implies the cross-state exclusivity that `bitvm3_graph.ivy` states
//! separately as `invariant [inv_013_a]`/`invariant [inv_013_b]` (`take1_outcome(G) ->
//! ~take2_outcome(G) & ~disprove_outcome(G)`, `take2_outcome(G) -> ~disprove_outcome(G)`): once
//! a graph reaches one of the four closed states, `allowed_graph_transition` forbids it from
//! ever reaching a different one.
//!
//! Real write-site citations:
//! - node/src/utils.rs:4472 (`update_graph_status`) is the primary setter, called throughout
//!   `refresh_graph` (node/src/utils.rs:769-...) as it scans Bitcoin/GOAT chain state
//!   (prekickoff/kickoff/take1/take2/challenge/disprove tx confirmations, watchtower-challenge
//!   and assert-commit timelines) to compute the graph's latest status. It skips a write only
//!   when the new status exactly equals the current one AND a default `ChallengeSubStatus` is
//!   supplied (utils.rs:4482-4490) -- a redundant-write optimization, not a check against
//!   `allowed_graph_transition`.
//! - node/src/scheduled_tasks/event_watch_task.rs:486-490 writes `Disprove` directly via
//!   `GraphUpdate`/`update_graph`, bypassing `update_graph_status` (and its redundant-write
//!   check) entirely, on a `GatewayDisprove` on-chain event.
//! - node/src/scheduled_tasks/event_watch_task.rs:934-941 (`handle_post_graph_data_events`)
//!   likewise writes `OperatorDataPushed` directly, bypassing `update_graph_status`, on a
//!   `PostGraphDataEvent`.
//!
//! None of these three write paths checks the row's current status against
//! `allowed_graph_transition` before writing.
//!
//! The six other `GraphStatus` variants (`Created`, `Presigned`, `L2Recorded`,
//! `OperatorKickOffing`, `Challenging`, `Disproving`) are display-only projections computed on
//! read (node/src/rpc_service/bitvm2.rs:745-762, crates/store/src/schema.rs:346-351) and are
//! never written to `Graph.status`, so -- mirroring `InstanceBridgeInStatus`'s excluded
//! display variants -- they are intentionally excluded here.

use store::GraphStatus;

/// The 11 persisted (non-display) graph states, used for bounded/exhaustive enumeration in
/// the Kani harnesses below.
pub const ALL_STATES: [GraphStatus; 11] = [
    GraphStatus::OperatorPresigned,
    GraphStatus::CommitteePresigned,
    GraphStatus::OperatorDataPushed,
    GraphStatus::PreKickoff,
    GraphStatus::OperatorKickOff,
    GraphStatus::Challenge,
    GraphStatus::Disprove,
    GraphStatus::Obsoleted,
    GraphStatus::Skipped,
    GraphStatus::OperatorTake1,
    GraphStatus::OperatorTake2,
];

/// Direct transcription of `bitvm3_graph.ivy`'s `axiom allowed_graph_transition(F,T)`,
/// including its `F = T` self-loop clause.
pub fn allowed_graph_transition(from: GraphStatus, to: GraphStatus) -> bool {
    use GraphStatus::*;
    from == to
        || matches!(
            (from, to),
            (OperatorPresigned, CommitteePresigned)
                | (CommitteePresigned, OperatorDataPushed)
                | (OperatorDataPushed, PreKickoff)
                | (OperatorDataPushed, Skipped)
                | (OperatorDataPushed, Obsoleted)
                | (OperatorPresigned, Obsoleted)
                | (CommitteePresigned, Obsoleted)
                | (PreKickoff, OperatorKickOff)
                | (PreKickoff, Skipped)
                | (Obsoleted, OperatorKickOff)
                | (Obsoleted, Skipped)
                | (OperatorKickOff, OperatorTake1)
                | (OperatorKickOff, Challenge)
                | (OperatorKickOff, Disprove)
                | (Challenge, Disprove)
                | (Challenge, OperatorTake2)
        )
}

/// States with no outgoing edge in the graph other than the self-loop. Equal to
/// `store::GraphStatus::get_closed_status()`; kept as a separate function so the property
/// harnesses below can check that equivalence rather than assume it.
pub fn is_terminal(status: GraphStatus) -> bool {
    use GraphStatus::*;
    matches!(status, OperatorTake1 | OperatorTake2 | Disprove | Skipped)
}

/// Mirrors the *actual* production mutation paths today: `update_graph_status`
/// (node/src/utils.rs:4472) and the two raw `GraphUpdate`/`update_graph` call sites in
/// `event_watch_task.rs` all write `to` unconditionally -- none of them checks the row's
/// current status against `allowed_graph_transition` first.
pub fn unguarded_set_status(_from: GraphStatus, to: GraphStatus) -> GraphStatus {
    to
}

/// A guarded setter that would close the gap: reject any transition not present in
/// `allowed_graph_transition`. Proposed remediation, not (yet) wired into any call site.
pub fn try_transition_graph_status(
    from: GraphStatus,
    to: GraphStatus,
) -> Result<GraphStatus, (GraphStatus, GraphStatus)> {
    if allowed_graph_transition(from, to) { Ok(to) } else { Err((from, to)) }
}

#[cfg(kani)]
mod verification {
    use super::*;

    /// Kani enumerates a bounded index and maps it onto the 11 persisted states so symbolic
    /// execution only ever sees valid enum values (avoids needing a `kani::Arbitrary` impl /
    /// derive on `GraphStatus`).
    fn any_status() -> GraphStatus {
        let idx: usize = kani::any();
        kani::assume(idx < ALL_STATES.len());
        ALL_STATES[idx]
    }

    /// Every state self-loops, per the Ivy axiom's `F = T` clause.
    #[kani::proof]
    fn self_loop_always_allowed() {
        let s = any_status();
        assert!(allowed_graph_transition(s, s));
    }

    /// `is_terminal` matches `GraphStatus::get_closed_status()` exactly, and terminal states
    /// have no real (non-self) outgoing edge -- this is the property that implies
    /// `bitvm3_graph.ivy`'s `inv_013_a`/`inv_013_b` mutual-exclusivity invariants (see module
    /// doc comment).
    #[kani::proof]
    fn closed_statuses_match_and_have_no_real_outgoing_edge() {
        let from = any_status();
        let to = any_status();
        assert_eq!(is_terminal(from), GraphStatus::get_closed_status().contains(&from));
        if is_terminal(from) && from != to {
            assert!(!allowed_graph_transition(from, to));
        }
    }

    /// The guarded setter can never produce a transition outside `allowed_graph_transition`.
    #[kani::proof]
    fn guarded_setter_never_violates_spec() {
        let from = any_status();
        let to = any_status();
        if let Ok(result) = try_transition_graph_status(from, to) {
            assert!(allowed_graph_transition(from, result));
        }
    }

    /// EXPECTED TO FAIL. This is the actual finding: it shows the real production write paths
    /// (`update_graph_status` and the two raw `GraphUpdate` call sites in
    /// `event_watch_task.rs`) can realize a transition `bitvm3_graph.ivy`'s own
    /// `allowed_graph_transition` axiom forbids -- e.g. jumping from `OperatorPresigned`
    /// straight to `OperatorTake1`, skipping committee presigning, kickoff, and challenge
    /// resolution entirely. `cargo kani` should report a concrete counterexample (from, to)
    /// pair for this harness.
    #[kani::proof]
    fn unguarded_setter_can_violate_spec() {
        let from = any_status();
        let to = any_status();
        let result = unguarded_set_status(from, to);
        assert!(allowed_graph_transition(from, result));
    }
}
