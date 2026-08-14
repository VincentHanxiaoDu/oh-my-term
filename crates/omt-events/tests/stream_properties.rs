//! Two properties the whole remote design rests on, over many shapes.
//!
//! Not example tests. Both of these are claims about *every* position and
//! *every* interleaving, and an example checks one — which is exactly how a
//! resume bug survives: the one position somebody thought to write down works.
//!
//! The generator is a linear congruential one seeded per case rather than a
//! property-testing crate. It is four lines, it shrinks by being deterministic
//! (a failing seed is printed and reproduces exactly), and it does not put a
//! dependency at L1 where the whole point is that there are almost none.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]

use omt_events::{
    Event, EventBus, EventPayload, EventSourceTag, Fanout, Filter, InstanceEvent, Policy,
    ResumeOutcome,
};
use omt_types::{Seq, SeqScope, SessionId, Timestamp};

/// A deterministic pseudo-random source.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

fn event(scope: SessionId, seq: u64) -> Event {
    Event {
        scope: SeqScope::Session { session: scope },
        seq: Seq::new(seq),
        ts: Timestamp::from_unix_seconds(0),
        source: EventSourceTag::Core,
        payload: EventPayload::Instance(InstanceEvent::Degraded {
            detail: format!("event {seq}"),
        }),
    }
}

#[test]
fn a_resume_yields_exactly_what_came_after_it_in_order_and_once() {
    // The claim every reconnect depends on. A duplicate is a card answered
    // twice; a gap is a card nobody ever sees; an inversion puts a tool result
    // above the call it belongs to.
    for seed in 0..64u64 {
        let mut rng = Lcg(seed.wrapping_mul(0x9e37_79b9).wrapping_add(1));
        let count = 1 + rng.below(200);
        // A window big enough that nothing is evicted: eviction has its own
        // property below, and mixing the two would let a pass on one hide a
        // failure on the other.
        let capacity = (count as usize) + 8;

        let scope = SessionId::new();
        let mut bus = EventBus::new(capacity);
        let subscriber = bus.subscribe(Filter::default(), Policy::default());
        for seq in 1..=count {
            bus.publish(event(scope, seq));
        }

        // Every position, including before the first and after the last.
        for after in 0..=count + 1 {
            let ResumeOutcome::Replayed { events } = bus.resume(subscriber, Seq::new(after)) else {
                panic!("seed {seed}: resume from {after} asked for a resync inside the window");
            };
            let got: Vec<u64> = events.iter().map(|e| e.seq.get()).collect();
            let expected: Vec<u64> = ((after + 1)..=count).collect();
            assert_eq!(
                got, expected,
                "seed {seed}: resuming after {after} of {count}"
            );
        }
    }
}

#[test]
fn a_position_the_window_has_passed_asks_for_a_resync_rather_than_answering_partially() {
    // The honest failure. A partial answer looks like a complete one, and the
    // client that received it believes it is caught up.
    for seed in 0..32u64 {
        let mut rng = Lcg(seed.wrapping_mul(0x2545_f491).wrapping_add(7));
        let capacity = 1 + rng.below(32) as usize;
        let overflow = capacity as u64 + 1 + rng.below(64);

        let scope = SessionId::new();
        let mut bus = EventBus::new(capacity);
        let subscriber = bus.subscribe(Filter::default(), Policy::default());
        for seq in 1..=overflow {
            bus.publish(event(scope, seq));
        }

        // Position 0 is definitely gone: more events were published than the
        // window holds.
        match bus.resume(subscriber, Seq::new(0)) {
            ResumeOutcome::Resync { from, .. } => {
                assert!(
                    from.get() > 1,
                    "seed {seed}: resync pointed at a position that was not lost"
                );
            }
            ResumeOutcome::Replayed { events } => panic!(
                "seed {seed}: {} events replayed from a window of {capacity} after {overflow} published",
                events.len()
            ),
        }
    }
}

#[test]
fn a_subscriber_that_reads_is_never_disconnected_however_long_it_runs() {
    // The other half of backpressure, and the one a naive counter gets wrong:
    // a client that keeps up must be able to run indefinitely. A queue that
    // only ever grows would disconnect the healthiest client eventually.
    for seed in 0..16u64 {
        let mut rng = Lcg(seed.wrapping_mul(0x1234_5677).wrapping_add(3));
        let scope = SessionId::new();
        let mut bus = EventBus::new(64);
        let subscriber = bus.subscribe(Filter::default(), Policy::default());

        for seq in 1..=5_000 {
            for (_, fanout) in bus.publish(event(scope, seq)) {
                assert!(
                    !matches!(fanout, Fanout::Disconnect { .. }),
                    "seed {seed}: a subscriber that keeps up was disconnected at {seq}"
                );
            }
            // It reads, at a rate that keeps up but not instantly.
            if rng.below(4) != 0 {
                bus.flushed(subscriber, 1);
            }
        }
    }
}

#[test]
fn a_subscriber_that_never_reads_is_disconnected_with_a_reason() {
    // Not buffered without bound. The alternative is an instance whose memory
    // grows because one phone went into a tunnel.
    let scope = SessionId::new();
    let mut bus = EventBus::new(1024);
    bus.subscribe(Filter::default(), Policy::default());

    let mut disconnected = None;
    for seq in 1..=100_000 {
        for (_, fanout) in bus.publish(event(scope, seq)) {
            if let Fanout::Disconnect { reason } = fanout {
                disconnected = Some(reason);
            }
        }
        if disconnected.is_some() {
            break;
        }
    }
    let reason = disconnected.expect("a subscriber that never reads ran forever");
    assert!(
        !reason.is_empty(),
        "disconnected with no reason, which a client cannot tell from a network fault"
    );
}

#[test]
fn what_a_subscriber_filtered_out_never_affects_whether_it_keeps_up() {
    // Per-subscription backpressure means per *wanted* traffic. A client
    // watching a quiet session while another floods must not be disconnected,
    // and the ordering inside `publish` is the only thing that guarantees it.
    for seed in 0..16u64 {
        let mut rng = Lcg(seed.wrapping_mul(0xabcd_1234).wrapping_add(11));
        let (quiet, noisy) = (SessionId::new(), SessionId::new());
        let mut bus = EventBus::new(4096);
        let watcher = bus.subscribe(
            Filter {
                sessions: vec![quiet],
                ..Filter::default()
            },
            Policy::default(),
        );

        for seq in 1..=20_000 {
            let scope = if rng.below(100) == 0 { quiet } else { noisy };
            for (_, fanout) in bus.publish(event(scope, seq)) {
                assert!(
                    !matches!(fanout, Fanout::Disconnect { .. }),
                    "seed {seed}: disconnected over another session's traffic"
                );
            }
            if scope == quiet {
                bus.flushed(watcher, 1);
            }
        }
    }
}
