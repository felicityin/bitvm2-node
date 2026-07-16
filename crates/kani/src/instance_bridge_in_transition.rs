//! Formalizes the intended `InstanceBridgeInStatus` transition graph for bridge-in
//! `Instance`s, and (under `#[cfg(kani)]`) proves properties about it with Kani.
//!
//! `bitvm3_instance.ivy` in the `bitvm-node-formal-verification` repo does not constrain
//! legal `bridge_in_status`/`bridge_out_status` transitions the way `bitvm3_graph.ivy` does
//! for `graph_status` (compare its `axiom allowed_graph_transition`). The graph below is a
//! reconstruction of the transition relation actually driven by the scheduled maintenance
//! tasks, not an existing spec -- it is the thing that *should* be true, so it can be checked
//! against what the code actually does.
//!
//! State diagram (matches `allowed_bridge_in_transition` exactly):
//!
//! ```text
//! UserIniting
//!     │
//!     ▼
//! UserInited
//!     ├──────────────► NoEnoughCommitteesAnswered
//!     ├──────────────► UserDiscarded
//!     │
//!     ▼
//! CommitteesAnswered
//!     ├──────────────► UserDiscarded
//!     │
//!     ▼
//! UserBroadcastPeginPrepare
//!     ├──────────────► PresignedFailed
//!     │
//!     ▼
//! Presigned
//!     ├──────────────► Timeout
//!     │
//!     ▼
//! RelayerL1Broadcasted
//!     ├──────────────► RelayerL2Minted
//!     └──────────────► RelayerL2MintedFailed
//!
//! PresignedFailed ───► Timeout ───► UserCanceled
//! ```
//!
//! Edge citations (node/src/scheduled_tasks/instance_maintenance_tasks.rs unless noted):
//! - UserIniting -> UserInited: created by node/src/rpc_service/handler/bitvm2_handler.rs:202
//!   (`UserIniting`), advanced on the on-chain `BridgeInRequest` event per schema.rs:198's
//!   doc comment ("UserInited, // from contract event request").
//! - UserInited -> CommitteesAnswered | NoEnoughCommitteesAnswered: lines 239-250
//!   (`instance_window_expiration_monitor`, gated on committee quorum size).
//! - CommitteesAnswered -> UserBroadcastPeginPrepare: lines 356-357 (`instance_btc_tx_monitor`).
//! - {UserInited, CommitteesAnswered} -> UserDiscarded: lines 429-451 (input UTXO spent
//!   elsewhere while waiting for BTC confirmation).
//! - UserBroadcastPeginPrepare -> Presigned: node/src/utils.rs:3889-3895 (`store_graph`,
//!   once `committee_pre_signed()`), and node/src/utils.rs:4498-4505 (`update_graph_status`,
//!   as a side effect of a `Graph` transitioning to `GraphStatus::CommitteePresigned`) --
//!   two independent writers reaching the same edge.
//! - UserBroadcastPeginPrepare -> PresignedFailed: lines 292-297 (`instance_expiration_monitor`,
//!   presign time expiry).
//! - Presigned -> RelayerL1Broadcasted: lines 359-361.
//! - {Presigned, PresignedFailed} -> Timeout: lines 313-322 (BTC lock-height expiry).
//! - Timeout -> UserCanceled: lines 363-364.
//! - RelayerL1Broadcasted -> RelayerL2Minted: node/src/scheduled_tasks/event_watch_task.rs:568-593.
//! - RelayerL1Broadcasted -> RelayerL2MintedFailed: inferred from the enum's existence
//!   (schema.rs:206); no corresponding write site was found in the current codebase.

use store::InstanceBridgeInStatus;

/// The 13 persisted (non-display) bridge-in states, used for bounded/exhaustive
/// enumeration in the Kani harnesses below. The remaining `InstanceBridgeInStatus`
/// variants (`Initiated`, `Verified`, `Submitted`, `Failed`, `Processing`, `Success`,
/// `Canceled`) are display-only projections computed on read
/// (node/src/rpc_service/bitvm2.rs:776-797) and are never written to `Instance.status`,
/// so they are intentionally excluded here.
pub const ALL_STATES: [InstanceBridgeInStatus; 13] = [
    InstanceBridgeInStatus::UserIniting,
    InstanceBridgeInStatus::UserInited,
    InstanceBridgeInStatus::CommitteesAnswered,
    InstanceBridgeInStatus::UserBroadcastPeginPrepare,
    InstanceBridgeInStatus::Presigned,
    InstanceBridgeInStatus::PresignedFailed,
    InstanceBridgeInStatus::RelayerL1Broadcasted,
    InstanceBridgeInStatus::RelayerL2Minted,
    InstanceBridgeInStatus::RelayerL2MintedFailed,
    InstanceBridgeInStatus::Timeout,
    InstanceBridgeInStatus::UserCanceled,
    InstanceBridgeInStatus::NoEnoughCommitteesAnswered,
    InstanceBridgeInStatus::UserDiscarded,
];

/// Intended transition graph for `Instance.status` while `is_bridge_in`. See the module
/// doc comment for per-edge citations.
pub fn allowed_bridge_in_transition(from: InstanceBridgeInStatus, to: InstanceBridgeInStatus) -> bool {
    use InstanceBridgeInStatus::*;
    matches!(
        (from, to),
        (UserIniting, UserInited)
            | (UserInited, CommitteesAnswered)
            | (UserInited, NoEnoughCommitteesAnswered)
            | (UserInited, UserDiscarded)
            | (CommitteesAnswered, UserBroadcastPeginPrepare)
            | (CommitteesAnswered, UserDiscarded)
            | (UserBroadcastPeginPrepare, Presigned)
            | (UserBroadcastPeginPrepare, PresignedFailed)
            | (Presigned, RelayerL1Broadcasted)
            | (Presigned, Timeout)
            | (PresignedFailed, Timeout)
            | (RelayerL1Broadcasted, RelayerL2Minted)
            | (RelayerL1Broadcasted, RelayerL2MintedFailed)
            | (Timeout, UserCanceled)
    )
}

/// States with no outgoing edge in the intended graph.
pub fn is_terminal(status: InstanceBridgeInStatus) -> bool {
    use InstanceBridgeInStatus::*;
    matches!(
        status,
        RelayerL2Minted
            | RelayerL2MintedFailed
            | NoEnoughCommitteesAnswered
            | UserCanceled
            | UserDiscarded
    )
}

/// Mirrors the *actual* production mutation path today: `InstanceUpdate::with_status`
/// (crates/store/src/localdb.rs) feeding `LocalDB::update_instance_status`
/// (crates/store/src/localdb.rs:893-914), which issues an unconditional
/// `UPDATE instance SET status = ? WHERE instance_id = ?` -- no check against the
/// row's current status is performed anywhere on this path.
pub fn unguarded_set_status(
    _from: InstanceBridgeInStatus,
    to: InstanceBridgeInStatus,
) -> InstanceBridgeInStatus {
    to
}

/// A guarded setter that would close the gap: reject any transition not present in
/// `allowed_bridge_in_transition`. Proposed remediation, not (yet) wired into
/// `crates/store/src/localdb.rs` or any call site.
pub fn try_transition_bridge_in_status(
    from: InstanceBridgeInStatus,
    to: InstanceBridgeInStatus,
) -> Result<InstanceBridgeInStatus, (InstanceBridgeInStatus, InstanceBridgeInStatus)> {
    if allowed_bridge_in_transition(from.clone(), to.clone()) {
        Ok(to)
    } else {
        Err((from, to))
    }
}

#[cfg(kani)]
mod verification {
    use super::*;

    /// Kani enumerates a bounded index and maps it onto the 13 persisted states so
    /// symbolic execution only ever sees valid enum values (avoids needing a
    /// `kani::Arbitrary` impl / derive on `InstanceBridgeInStatus`).
    fn any_status() -> InstanceBridgeInStatus {
        let idx: usize = kani::any();
        kani::assume(idx < ALL_STATES.len());
        ALL_STATES[idx].clone()
    }

    /// The intended graph is internally consistent: `RelayerL2Minted` (pegBTC actually
    /// minted on GOAT) is only reachable directly from `RelayerL1Broadcasted`, i.e. every
    /// path into it has already passed through `CommitteesAnswered`. No edge can skip
    /// committee attestation.
    #[kani::proof]
    fn minted_requires_prior_committee_answer() {
        let from = any_status();
        let to = any_status();
        if allowed_bridge_in_transition(from.clone(), to.clone())
            && to == InstanceBridgeInStatus::RelayerL2Minted
        {
            assert_eq!(from, InstanceBridgeInStatus::RelayerL1Broadcasted);
        }
    }

    /// Terminal states never have an outgoing edge in the intended graph.
    #[kani::proof]
    fn terminal_states_are_absorbing() {
        let from = any_status();
        let to = any_status();
        if is_terminal(from.clone()) {
            assert!(!allowed_bridge_in_transition(from, to));
        }
    }

    /// The guarded setter can never produce a transition outside the intended graph.
    /// (Mostly a regression guard: keeps `try_transition_bridge_in_status` honest if its
    /// implementation is later refactored to be more than a direct table lookup.)
    #[kani::proof]
    fn guarded_setter_never_violates_spec() {
        let from = any_status();
        let to = any_status();
        if let Ok(result) = try_transition_bridge_in_status(from.clone(), to) {
            assert!(allowed_bridge_in_transition(from, result));
        }
    }

    /// EXPECTED TO FAIL. This is the actual finding: it shows the real production
    /// mutation path (unconditional `UPDATE instance SET status = ?`,
    /// crates/store/src/localdb.rs:893-914) can realize a transition the intended graph
    /// forbids -- e.g. jumping from `UserIniting` straight to `RelayerL2Minted`, skipping
    /// committee attestation and BTC confirmation entirely. `cargo kani` should report a
    /// concrete counterexample (from, to) pair for this harness.
    #[kani::proof]
    fn unguarded_setter_can_violate_spec() {
        let from = any_status();
        let to = any_status();
        let result = unguarded_set_status(from.clone(), to);
        assert!(allowed_bridge_in_transition(from, result));
    }
}
