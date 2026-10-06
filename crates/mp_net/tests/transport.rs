//! `SimNet`'s link conditions as documented (SPEC 9.3): latency and jitter
//! bound delivery times both ways, loss only touches the unreliable
//! channel, reliable delivery keeps order, cuts and closes end a link on
//! both sides once, and a seed replays a run exactly.

use mp_net::transport::{Channel, Conditions, NetEvent, SimEnd, SimNet, Transport};

fn poll(e: &mut SimEnd) -> Vec<NetEvent> {
    let mut ev = Vec::new();
    e.poll(&mut ev);
    ev
}

fn bytes(e: &mut SimEnd) -> Vec<u8> {
    poll(e)
        .into_iter()
        .filter_map(|x| match x {
            NetEvent::Message(_, b) => Some(b[0]),
            _ => None,
        })
        .collect()
}

const SLOW: Conditions = Conditions {
    latency: 30.0,
    jitter: 20.0,
    loss: 0.0,
};

#[test]
fn delivery_waits_for_latency_and_never_exceeds_latency_plus_jitter_both_ways() {
    let net = SimNet::new(2);
    let mut h = net.host();
    let mut c = net.connect(SLOW);
    poll(&mut h);
    poll(&mut c);
    for i in 0..100u8 {
        c.send(0, Channel::Unreliable, &[i]);
        h.send(c.id(), Channel::Unreliable, &[i]);
    }
    net.advance_to(29.999);
    assert!(bytes(&mut h).is_empty() && bytes(&mut c).is_empty());
    net.advance_to(50.0);
    assert_eq!(bytes(&mut h).len(), 100);
    assert_eq!(bytes(&mut c).len(), 100);
    assert_eq!(net.in_flight(), 0);
}

#[test]
fn without_jitter_messages_arrive_exactly_on_time_in_send_order() {
    let net = SimNet::new(3);
    let mut h = net.host();
    let mut c = net.connect(Conditions {
        latency: 10.0,
        jitter: 0.0,
        loss: 0.0,
    });
    poll(&mut h);
    for i in 0..20u8 {
        c.send(0, Channel::Unreliable, &[i]);
    }
    net.advance_to(9.0);
    assert!(bytes(&mut h).is_empty());
    assert_eq!(net.in_flight(), 20);
    net.advance_to(10.0);
    assert_eq!(bytes(&mut h), (0..20).collect::<Vec<u8>>());
}

#[test]
fn loss_drops_only_unreliable_messages() {
    let net = SimNet::new(4);
    let mut h = net.host();
    let mut c = net.connect(Conditions {
        latency: 5.0,
        jitter: 0.0,
        loss: 1.0,
    });
    poll(&mut h);
    for i in 0..50u8 {
        c.send(0, Channel::Unreliable, &[i]);
        c.send(0, Channel::Reliable, &[100 + i]);
        h.send(c.id(), Channel::Unreliable, &[i]);
    }
    net.advance_to(100.0);
    assert_eq!(bytes(&mut h), (100..150).collect::<Vec<u8>>());
    assert!(
        bytes(&mut c).is_empty(),
        "the host's sends cross the same link"
    );
}

#[test]
fn loss_is_about_the_rate_asked_for() {
    let net = SimNet::new(5);
    let mut h = net.host();
    let mut c = net.connect(Conditions {
        latency: 1.0,
        jitter: 0.0,
        loss: 0.3,
    });
    for _ in 0..4000 {
        c.send(0, Channel::Unreliable, &[0]);
    }
    net.advance_to(10.0);
    let got = bytes(&mut h).len() as f64 / 4000.0;
    assert!((got - 0.7).abs() < 0.03, "{got}");
}

#[test]
fn reliable_messages_keep_their_order_across_a_change_of_conditions() {
    let net = SimNet::new(6);
    let mut h = net.host();
    let mut c = net.connect(SLOW);
    poll(&mut h);
    c.send(0, Channel::Reliable, &[1]);
    // The link gets much faster: later messages can't overtake.
    net.set_conditions(
        &c,
        Conditions {
            latency: 1.0,
            jitter: 0.0,
            loss: 0.0,
        },
    );
    c.send(0, Channel::Reliable, &[2]);
    net.advance_to(10.0);
    assert!(bytes(&mut h).is_empty(), "2 waits behind 1");
    net.advance_to(60.0);
    assert_eq!(bytes(&mut h), [1, 2]);
}

#[test]
fn reliable_messages_keep_their_order_when_the_link_becomes_perfect() {
    let net = SimNet::new(6);
    let mut h = net.host();
    let mut c = net.connect(SLOW);
    poll(&mut h);
    c.send(0, Channel::Reliable, &[1]);
    net.set_conditions(&c, Conditions::PERFECT);
    c.send(0, Channel::Reliable, &[2]);
    net.advance_to(60.0);
    assert_eq!(bytes(&mut h), [1, 2]);
}

#[test]
fn closing_from_either_side_disconnects_both_once() {
    let net = SimNet::new(7);
    let mut h = net.host();
    let mut a = net.connect(Conditions::LAN);
    let mut b = net.connect(Conditions::LAN);
    poll(&mut h);
    poll(&mut a);
    poll(&mut b);
    // The host kicks a; b hangs up itself.
    h.close(a.id());
    b.close(0);
    b.close(0);
    net.cut(&b);
    h.close(b.id());
    let ev = poll(&mut h);
    assert_eq!(
        ev,
        [
            NetEvent::Disconnected(a.id()),
            NetEvent::Disconnected(b.id())
        ]
    );
    assert_eq!(poll(&mut a), [NetEvent::Disconnected(0)]);
    assert_eq!(poll(&mut b), [NetEvent::Disconnected(0)]);
    // Nothing gets through a closed link either way.
    h.send(a.id(), Channel::Reliable, &[1]);
    a.send(0, Channel::Reliable, &[1]);
    net.advance_to(100.0);
    assert!(poll(&mut h).is_empty() && poll(&mut a).is_empty());
}

#[test]
fn a_cut_drops_both_directions_in_flight_but_spares_other_links() {
    let net = SimNet::new(8);
    let mut h = net.host();
    let mut a = net.connect(SLOW);
    let mut b = net.connect(SLOW);
    poll(&mut h);
    poll(&mut a);
    poll(&mut b);
    h.send(a.id(), Channel::Reliable, &[1]);
    h.send(b.id(), Channel::Reliable, &[2]);
    a.send(0, Channel::Reliable, &[3]);
    b.send(0, Channel::Reliable, &[4]);
    assert_eq!(net.in_flight(), 4);
    net.cut(&a);
    assert_eq!(net.in_flight(), 2);
    net.advance_to(100.0);
    assert_eq!(poll(&mut a), [NetEvent::Disconnected(0)]);
    assert_eq!(bytes(&mut b), [2]);
    let ev = poll(&mut h);
    assert!(ev.contains(&NetEvent::Disconnected(a.id())));
    assert!(ev.contains(&NetEvent::Message(b.id(), vec![4])));
}

#[test]
fn a_new_connection_gets_a_new_peer_id_and_unknown_peers_are_ignored() {
    let net = SimNet::new(9);
    let mut h = net.host();
    let a = net.connect(Conditions::PERFECT);
    net.cut(&a);
    let mut a2 = net.connect(Conditions::PERFECT);
    assert_ne!(a.id(), a2.id());
    assert_eq!(h.id(), 0);
    assert_eq!(
        poll(&mut h),
        [
            NetEvent::Connected(a.id()),
            NetEvent::Disconnected(a.id()),
            NetEvent::Connected(a2.id())
        ]
    );
    // Sends and closes to ids that never were do nothing (and don't panic).
    h.send(99, Channel::Reliable, &[1]);
    h.close(99);
    // The old id is dead even though a new one is up.
    h.send(a.id(), Channel::Reliable, &[2]);
    h.send(a2.id(), Channel::Reliable, &[3]);
    assert_eq!(
        poll(&mut a2),
        [NetEvent::Connected(0), NetEvent::Message(0, vec![3])]
    );
}

#[test]
fn time_only_moves_forward() {
    let net = SimNet::new(10);
    let mut h = net.host();
    let mut c = net.connect(Conditions {
        latency: 20.0,
        jitter: 0.0,
        loss: 0.0,
    });
    poll(&mut h);
    net.advance_to(50.0);
    net.advance_to(10.0);
    assert_eq!(net.now(), 50.0);
    c.send(0, Channel::Reliable, &[1]);
    net.advance_to(69.0);
    assert!(bytes(&mut h).is_empty(), "due at 70, from 50");
    net.advance_to(70.0);
    assert_eq!(bytes(&mut h), [1]);
}

/// The same seed and sends give the same arrivals; another seed doesn't.
#[test]
fn a_seed_replays_a_lossy_jittery_run_exactly() {
    let run = |seed: u32| {
        let net = SimNet::new(seed);
        let mut h = net.host();
        let mut c = net.connect(Conditions {
            latency: 15.0,
            jitter: 40.0,
            loss: 0.2,
        });
        let mut log = Vec::new();
        for t in 0..300u32 {
            c.send(0, Channel::Unreliable, &[t as u8]);
            if t % 3 == 0 {
                h.send(c.id(), Channel::Reliable, &[t as u8]);
            }
            net.advance_to(t as f64);
            log.push((t, bytes(&mut h), bytes(&mut c)));
        }
        log
    };
    assert_eq!(run(11), run(11));
    assert_ne!(run(11), run(12));
}

/// A boxed transport passes everything through.
#[test]
fn a_boxed_transport_is_a_transport() {
    let net = SimNet::new(13);
    let mut h: Box<dyn Transport> = Box::new(net.host());
    let mut c: Box<dyn Transport> = Box::new(net.connect(Conditions::PERFECT));
    c.send(0, Channel::Reliable, b"x");
    let mut ev = Vec::new();
    h.poll(&mut ev);
    assert_eq!(
        ev,
        [NetEvent::Connected(1), NetEvent::Message(1, b"x".to_vec())]
    );
    h.close(1);
    let mut ev = Vec::new();
    c.poll(&mut ev);
    assert_eq!(ev, [NetEvent::Connected(0), NetEvent::Disconnected(0)]);
}
