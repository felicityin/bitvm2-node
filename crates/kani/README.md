# kani-harness

[Kani](https://github.com/model-checking/kani) is a bounded model checker for Rust (built on
CBMC). Given a `#[kani::proof]` harness, it symbolically explores all inputs up to the
configured bounds and either proves the harness's assertions hold for every input, or returns
a concrete counterexample when they don't.

This project uses Kani to check **implementation-level** properties that the protocol-level
formal spec in [bitvm-node-formal-verification](https://github.com/GOATNetwork/bitvm-node-formal-verification) (Ivy) does not cover -- e.g. whether a specific
Rust state-transition function actually enforces the state machine the protocol assumes.

Harnesses live here, in this dedicated workspace member (package name `kani-harness`), rather
than inside the crates they check. This crate depends on those crates as an ordinary path
dependency (e.g. `store`) and re-models the logic under test as pure functions Kani can
execute. Current harness sets, both covering `Instance.status` transitions on the same
`Instance` type (`crates/store/src/schema.rs`):

- [`src/instance_bridge_in_transition.rs`](src/instance_bridge_in_transition.rs) -- bridge-in
  (`InstanceBridgeInStatus`, 13 states).
- [`src/instance_bridge_out_transition.rs`](src/instance_bridge_out_transition.rs) -- bridge-out
  (`InstanceBridgeOutStatus`, 4 states).

## Installing Kani

Kani is not a workspace dependency; it's a separate toolchain installed once per machine.

```bash
cargo install --locked kani-verifier
cargo kani setup   # downloads the pinned nightly toolchain, CBMC, and the SAT solver
```

`cargo kani setup` installs into `~/.kani/kani-<version>` and does not touch
`rust-toolchain.toml` or the workspace's normal build. Re-run `cargo install --locked
kani-verifier` after a Kani version bump; `cargo kani setup` is idempotent.

Check the install:

```bash
cargo kani --version
```

## Running harnesses

Run every harness in this crate:

```bash
cargo kani -p kani-harness
```

Run a single harness:

```bash
cargo kani -p kani-harness --harness unguarded_setter_can_violate_spec
```

Harness names are the function name annotated with `#[kani::proof]` (no path qualification
needed, as long as it's unique in the crate). `--harness` matches by name across *all* modules,
so e.g. `unguarded_setter_can_violate_spec` currently matches one harness in
`instance_bridge_in_transition` **and** one in `instance_bridge_out_transition` (both run). Use the fully
qualified path (e.g. `--harness instance_bridge_out_transition::verification::unguarded_setter_can_violate_spec`)
to isolate one. Each run prints a per-check `SUCCESS`/`FAILURE` table and a final
`VERIFICATION:- SUCCESSFUL` or `VERIFICATION:- FAILED` line per harness.

## Getting a concrete counterexample

When a harness fails, Kani can emit a runnable Rust unit test that replays the exact failing
input:

```bash
cargo kani -p kani-harness --harness unguarded_setter_can_violate_spec \
  -Z concrete-playback --concrete-playback=print
```

This prints a `#[test] fn kani_concrete_playback_...()` block with the concrete byte values
that trigger the assertion failure. Swap `=print` for `=inplace` to have Kani insert that test
directly into the source file instead of just printing it. `-Z concrete-playback` is required
because the flag is still unstable in Kani 0.67.

## Where harnesses live

By convention, harnesses live in this crate, not inside the crate whose logic they check --
e.g. `src/instance_bridge_in_transition.rs` depends on `store` as a path dependency and models the
transition logic as pure functions, rather than adding `#[cfg(kani)]` code directly to
`crates/store`. Within this crate, each file groups the pure model it defines together with the
harnesses that check it, gated behind `#[cfg(kani)]`, e.g.:

```rust
#[cfg(kani)]
mod verification {
    use super::*;

    #[kani::proof]
    fn some_property_holds() {
        let x = any_bounded_value();
        assert!(invariant(x));
    }
}
```

`#[cfg(kani)]` is only ever true when compiling under `cargo kani`; a normal `cargo build`/`cargo
check` elides the module entirely, so Kani harnesses carry no runtime or compile-time cost for
the production binary. Because `kani` is not a real Cargo `cfg`, `Cargo.toml` sets:

```toml
[lints.rust]
unexpected_cfgs = { level = "allow", check-cfg = ["cfg(kani)"] }
```

to silence the `unexpected_cfgs` lint under a normal build. Any other crate that grows its own
`#[cfg(kani)]` code needs the same entry.

The `kani` crate itself (`kani::proof`, `kani::any`, `kani::assume`, ...) is injected
automatically by `cargo kani` -- it must **not** be added as a dependency in `Cargo.toml`, or a
normal build will fail to resolve it. This is also why this crate is named `kani-harness`
rather than `kani`: naming a workspace member `kani` would collide with that auto-injected
crate name once compiled under `cargo kani`.

## Adding a new harness

1. Identify a pure, synchronous function (or a small model of one) to check. Kani cannot
   execute `async fn`, real I/O, or a live database connection -- for code that touches those
   (e.g. `LocalDB::update_instance_status`, which issues a real SQL `UPDATE`), write a pure
   function in this crate that mirrors its observable contract instead of calling it directly,
   and say so in a comment (see `unguarded_set_status` in `instance_bridge_in_transition.rs` for the
   pattern).
2. For enums without a `kani::Arbitrary` impl, avoid deriving it just for the harness; instead
   enumerate the valid values into a `const` array and pick one with a bounded `kani::any()`
   index (see `any_status()` in `instance_bridge_in_transition.rs`). This keeps the harness from ever
   exploring invalid enum bit patterns.
3. Write the property as a plain `assert!`/`assert_eq!` inside a `#[kani::proof]` fn.
4. If the crate under test isn't already a dependency, add it as a path dependency in
   `Cargo.toml`.
5. Run it (`cargo kani -p kani-harness --harness <name>`) and confirm it either verifies, or
   fails with the counterexample you expect.

## Known limitations of the current harness set

- Both `instance_bridge_in_transition.rs` (13 states, 169 pairs) and `instance_bridge_out_transition.rs`
  (4 states, 16 pairs) have state spaces small enough that exhaustive unit tests would catch
  the same bugs. Kani's advantage shows up once a harness reasons over unbounded/numeric inputs
  (e.g. `committee_quorum_size <= committees_answers.len()`, amounts, block heights, the
  `bridge_out_lock_time` deadline check); that's the natural next harness to add.
- Both transition graphs are reconstructions from the scheduled maintenance/event-watch tasks,
  not an existing spec. In `instance_bridge_in_transition.rs`, one edge
  (`RelayerL1Broadcasted -> RelayerL2MintedFailed`) is inferred from the enum's existence rather
  than an observed write site. Treat both graphs as working hypotheses to validate against the
  team, not ground truth.
- Neither `try_transition_bridge_in_status` nor `try_transition_bridge_out_status` (the
  proposed guarded setters) is wired into any real call site yet. The harnesses prove
  properties about the proposed fix and about the current gap side by side; they don't close
  the gap in production code.
- `instance_bridge_out_transition.rs`'s `Initialize -> Timeout` edge is only enforced by
  `instance_bridge_out_monitor` for instances with `escrow_hash` set (it queries
  `escrow_hash IS NOT NULL`); the harness set does not currently model that precondition
  separately, so it treats the edge as unconditionally intended.
