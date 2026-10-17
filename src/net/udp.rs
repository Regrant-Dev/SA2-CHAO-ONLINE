//! UDP transport (direct, port forwarding, Tailscale/ZeroTier or LAN).
//!
//! Packet: `magic u32 | version u8 | kind u8 | epoch u32 | seq u32 | payload`.
//! - Unreliable: delivered as-is, may be lost or reordered (player state).
//! - Reliable: acked, resent, delivered in order, split into fragments when large.
//! - `epoch` is random per endpoint, so a restarted peer on the same address gets a fresh link.
//!
//! Non-blocking; call `poll` every frame. No threads.

use std::collections::{BTreeMap, HashMap};
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use super::wire::{Reader, WireError, Writer};

const MAGIC: u32 = u32::from_le_bytes(*b"SA2O");
const VERSION: u8 = 1;
const HEADER: usize = 14;
/// Safe UDP payload size across the internet and VPNs.
pub const MAX_PACKET: usize = 1200;
const FRAG_HEADER: usize = 2;
const MAX_FRAG_DATA: usize = MAX_PACKET - HEADER - FRAG_HEADER;
const MAX_FRAGMENTS: usize = 32;
/// Largest reliable message (~37 KB).
pub const MAX_RELIABLE: usize = MAX_FRAG_DATA * MAX_FRAGMENTS;
/// Largest unreliable message.
pub const MAX_UNRELIABLE: usize = MAX_PACKET - HEADER;

const RESEND_AFTER: Duration = Duration::from_millis(150);
const MAX_TRIES: u32 = 40;
/// How far ahead of the next expected sequence we buffer out-of-order packets.
const REORDER_WINDOW: u32 = 256;
/// Links to unknown addresses we accept; the session layer drops unwanted ones.
const MAX_LINKS: usize = 16;
/// A link that has heard nothing this long and has nothing left to deliver is dropped (the
/// session gave up on it long before), so stray packets can't fill the table.
const LINK_IDLE: Duration = Duration::from_secs(20);

mod kind {
    pub const UNRELIABLE: u8 = 0;
    pub const RELIABLE: u8 = 1;
    pub const ACK: u8 = 2;
}

#[derive(Debug, PartialEq)]
pub enum Received {
    Message {
        data: Vec<u8>,
        reliable: bool,
    },
    /// Reliable data could not be delivered or the peer broke the framing; the link is gone.
    LinkFailed,
}

struct Pending {
    packet: Vec<u8>,
    last_sent: Instant,
    tries: u32,
}

struct Link {
    remote_epoch: Option<u32>,
    next_send_seq: u32,
    unacked: BTreeMap<u32, Pending>,
    next_recv_seq: u32,
    out_of_order: BTreeMap<u32, Vec<u8>>,
    fragments: Vec<u8>,
    next_fragment: u8,
    /// When the peer last sent us anything.
    last_heard: Option<Instant>,
}

impl Link {
    fn new() -> Self {
        Self {
            remote_epoch: None,
            next_send_seq: 0,
            unacked: BTreeMap::new(),
            next_recv_seq: 0,
            out_of_order: BTreeMap::new(),
            fragments: Vec::new(),
            next_fragment: 0,
            last_heard: None,
        }
    }

    /// Resets the receive side for a new remote epoch.
    fn reset_receive(&mut self, epoch: u32) {
        self.remote_epoch = Some(epoch);
        self.next_recv_seq = 0;
        self.out_of_order.clear();
        self.fragments.clear();
        self.next_fragment = 0;
    }
}

pub struct Endpoint {
    socket: UdpSocket,
    epoch: u32,
    links: HashMap<SocketAddr, Link>,
    /// Test hook: returns true to drop an outgoing packet.
    pub drop_outgoing: Option<Box<dyn FnMut() -> bool + Send>>,
}

impl Endpoint {
    pub fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            epoch: random_epoch(),
            links: HashMap::new(),
            drop_outgoing: None,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.socket
            .local_addr()
            .expect("bound socket has an address")
    }

    /// Forgets a peer (after Bye/timeout/reject). Pending reliable data is dropped.
    pub fn remove(&mut self, addr: SocketAddr) {
        self.links.remove(&addr);
    }

    pub fn send_unreliable(&mut self, to: SocketAddr, data: &[u8]) {
        if data.len() > MAX_UNRELIABLE {
            return;
        }
        let packet = self.packet(kind::UNRELIABLE, 0, data);
        self.raw_send(to, &packet);
    }

    pub fn send_reliable(&mut self, to: SocketAddr, data: &[u8], now: Instant) {
        if data.len() > MAX_RELIABLE {
            return;
        }
        let chunks: Vec<&[u8]> = if data.is_empty() {
            vec![&[][..]]
        } else {
            data.chunks(MAX_FRAG_DATA).collect()
        };
        let count = chunks.len() as u8;
        for (index, chunk) in chunks.into_iter().enumerate() {
            let link = self.links.entry(to).or_insert_with(Link::new);
            let seq = link.next_send_seq;
            link.next_send_seq = link.next_send_seq.wrapping_add(1);
            let mut payload = Vec::with_capacity(FRAG_HEADER + chunk.len());
            payload.extend_from_slice(&[index as u8, count]);
            payload.extend_from_slice(chunk);
            let packet = self.packet(kind::RELIABLE, seq, &payload);
            self.raw_send(to, &packet);
            let link = self.links.get_mut(&to).expect("inserted above");
            link.unacked.insert(
                seq,
                Pending {
                    packet,
                    last_sent: now,
                    tries: 1,
                },
            );
        }
    }

    /// Receives everything waiting on the socket and resends overdue reliable packets.
    pub fn poll(&mut self, now: Instant) -> Vec<(SocketAddr, Received)> {
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => self.handle_packet(from, &buf[..n], now, &mut out),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                // Windows reports ICMP port-unreachable as ConnectionReset on UDP; ignore it.
                Err(e) if e.kind() == ErrorKind::ConnectionReset => continue,
                Err(_) => break,
            }
        }
        self.resend(now, &mut out);
        out
    }

    fn handle_packet(
        &mut self,
        from: SocketAddr,
        packet: &[u8],
        now: Instant,
        out: &mut Vec<(SocketAddr, Received)>,
    ) {
        let Ok((kind, epoch, seq, payload)) = parse_header(packet) else {
            return;
        };
        if !self.links.contains_key(&from) {
            if self.links.len() >= MAX_LINKS {
                return;
            }
            self.links.insert(from, Link::new());
        }
        let link = self.links.get_mut(&from).expect("inserted above");
        link.last_heard = Some(now);
        match link.remote_epoch {
            None => link.reset_receive(epoch),
            // The peer restarted: it lost everything, so start both directions over.
            Some(known) if known != epoch => {
                link.reset_receive(epoch);
                link.next_send_seq = 0;
                link.unacked.clear();
            }
            Some(_) => {}
        }

        match kind {
            kind::UNRELIABLE => {
                out.push((
                    from,
                    Received::Message {
                        data: payload.to_vec(),
                        reliable: false,
                    },
                ));
            }
            kind::ACK => {
                if let Some(link) = self.links.get_mut(&from) {
                    link.unacked.remove(&seq);
                }
            }
            kind::RELIABLE => {
                let ahead = seq.wrapping_sub(link.next_recv_seq);
                // Too far ahead: not kept, so not acked either (it's sent again later). A
                // duplicate of something delivered is acked again and dropped.
                let duplicate = ahead > u32::MAX / 2;
                if ahead >= REORDER_WINDOW && !duplicate {
                    return;
                }
                let ack = self.packet(kind::ACK, seq, &[]);
                self.raw_send(from, &ack);
                if duplicate {
                    return;
                }
                let link = self.links.get_mut(&from).expect("inserted above");
                link.out_of_order.insert(seq, payload.to_vec());
                while let Some(payload) = link.out_of_order.remove(&link.next_recv_seq) {
                    link.next_recv_seq = link.next_recv_seq.wrapping_add(1);
                    match reassemble(link, &payload) {
                        Ok(Some(data)) => out.push((
                            from,
                            Received::Message {
                                data,
                                reliable: true,
                            },
                        )),
                        Ok(None) => {}
                        Err(_) => {
                            self.links.remove(&from);
                            out.push((from, Received::LinkFailed));
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn resend(&mut self, now: Instant, out: &mut Vec<(SocketAddr, Received)>) {
        let mut failed = Vec::new();
        let mut resends = Vec::new();
        for (addr, link) in &mut self.links {
            for pending in link.unacked.values_mut() {
                if now.duration_since(pending.last_sent) < RESEND_AFTER {
                    continue;
                }
                if pending.tries >= MAX_TRIES {
                    failed.push(*addr);
                    break;
                }
                pending.tries += 1;
                pending.last_sent = now;
                resends.push((*addr, pending.packet.clone()));
            }
        }
        for (addr, packet) in resends {
            self.raw_send(addr, &packet);
        }
        for addr in failed {
            self.links.remove(&addr);
            out.push((addr, Received::LinkFailed));
        }
        self.links.retain(|_, link| {
            !link.unacked.is_empty()
                || link
                    .last_heard
                    .is_none_or(|at| now.duration_since(at) < LINK_IDLE)
        });
    }

    fn packet(&self, kind: u8, seq: u32, payload: &[u8]) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(MAGIC)
            .u8(VERSION)
            .u8(kind)
            .u32(self.epoch)
            .u32(seq)
            .bytes(payload);
        w.buf
    }

    fn raw_send(&mut self, to: SocketAddr, packet: &[u8]) {
        if let Some(drop) = self.drop_outgoing.as_mut()
            && drop()
        {
            return;
        }
        let _ = self.socket.send_to(packet, to);
    }
}

fn parse_header(packet: &[u8]) -> Result<(u8, u32, u32, &[u8]), WireError> {
    let mut r = Reader::new(packet);
    if r.u32()? != MAGIC {
        return Err(WireError::BadMagic);
    }
    let version = r.u8()?;
    if version != VERSION {
        return Err(WireError::BadVersion(version));
    }
    let kind = r.u8()?;
    let epoch = r.u32()?;
    let seq = r.u32()?;
    Ok((kind, epoch, seq, r.rest()))
}

/// Feeds one reliable payload into the link's fragment buffer.
/// Returns the full message when its last fragment arrives.
fn reassemble(link: &mut Link, payload: &[u8]) -> Result<Option<Vec<u8>>, WireError> {
    let mut r = Reader::new(payload);
    let index = r.u8()?;
    let count = r.u8()?;
    if count == 0 || count as usize > MAX_FRAGMENTS || index >= count || index != link.next_fragment
    {
        return Err(WireError::BadValue("fragment"));
    }
    link.fragments.extend_from_slice(r.rest());
    if index + 1 == count {
        link.next_fragment = 0;
        Ok(Some(std::mem::take(&mut link.fragments)))
    } else {
        link.next_fragment += 1;
        Ok(None)
    }
}

fn random_epoch() -> u32 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h = RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    h.write_u128(nanos);
    h.write_u32(std::process::id());
    h.finish() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn local() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
    }

    /// Polls both endpoints until `want` messages arrive at `b`, advancing fake time.
    fn pump(a: &mut Endpoint, b: &mut Endpoint, want: usize) -> Vec<(Vec<u8>, bool)> {
        let mut got = Vec::new();
        let mut now = Instant::now();
        for _ in 0..2000 {
            now += Duration::from_millis(20);
            a.poll(now);
            for (_, r) in b.poll(now) {
                if let Received::Message { data, reliable } = r {
                    got.push((data, reliable));
                }
            }
            if got.len() >= want {
                break;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        got
    }

    #[test]
    fn unreliable_and_reliable_delivery() {
        let mut a = Endpoint::bind(local()).unwrap();
        let mut b = Endpoint::bind(local()).unwrap();
        let to = b.local_addr();
        a.send_unreliable(to, b"state");
        a.send_reliable(to, b"hello", Instant::now());
        let got = pump(&mut a, &mut b, 2);
        assert!(got.contains(&(b"state".to_vec(), false)));
        assert!(got.contains(&(b"hello".to_vec(), true)));
    }

    #[test]
    fn large_reliable_message_is_fragmented_and_reassembled() {
        let mut a = Endpoint::bind(local()).unwrap();
        let mut b = Endpoint::bind(local()).unwrap();
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        a.send_reliable(b.local_addr(), &big, Instant::now());
        let got = pump(&mut a, &mut b, 1);
        assert_eq!(got, vec![(big, true)]);
    }

    #[test]
    fn reliable_survives_heavy_loss_in_order() {
        let mut a = Endpoint::bind(local()).unwrap();
        let mut b = Endpoint::bind(local()).unwrap();
        // Drop ~40% of packets in both directions (data and acks).
        let mut n = 0u32;
        a.drop_outgoing = Some(Box::new(move || {
            n = n.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (n >> 16) % 10 < 4
        }));
        let mut m = 7u32;
        b.drop_outgoing = Some(Box::new(move || {
            m = m.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (m >> 16) % 10 < 4
        }));
        let to = b.local_addr();
        let now = Instant::now();
        for i in 0..50u8 {
            a.send_reliable(to, &[i; 3000][..], now);
        }
        let got = pump(&mut a, &mut b, 50);
        assert_eq!(got.len(), 50);
        for (i, (data, reliable)) in got.iter().enumerate() {
            assert!(*reliable);
            assert_eq!(data, &vec![i as u8; 3000]);
        }
    }

    #[test]
    fn gives_up_on_dead_peer() {
        let mut a = Endpoint::bind(local()).unwrap();
        let dead = Endpoint::bind(local()).unwrap().local_addr(); // dropped: nobody listens
        let mut now = Instant::now();
        a.send_reliable(dead, b"anyone?", now);
        let mut failed = false;
        for _ in 0..MAX_TRIES + 2 {
            now += RESEND_AFTER;
            failed |= a
                .poll(now)
                .iter()
                .any(|(addr, r)| *addr == dead && *r == Received::LinkFailed);
        }
        assert!(failed);
    }

    #[test]
    fn ignores_garbage_packets() {
        let mut b = Endpoint::bind(local()).unwrap();
        let s = UdpSocket::bind(local()).unwrap();
        for junk in [&b"short"[..], &[0u8; 14], &[0xFF; 1500]] {
            s.send_to(junk, b.local_addr()).unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
        assert!(b.poll(Instant::now()).is_empty());
    }

    #[test]
    fn restarted_peer_gets_fresh_sequence() {
        let mut b = Endpoint::bind(local()).unwrap();
        let mut a1 = Endpoint::bind(local()).unwrap();
        let addr = a1.local_addr();
        a1.send_reliable(b.local_addr(), b"first", Instant::now());
        assert_eq!(pump(&mut a1, &mut b, 1).len(), 1);
        drop(a1);
        // Same address, new endpoint (new epoch, seq restarts at 0).
        let mut a2 = Endpoint::bind(addr).unwrap();
        a2.send_reliable(b.local_addr(), b"second", Instant::now());
        assert_eq!(pump(&mut a2, &mut b, 1), vec![(b"second".to_vec(), true)]);
    }

    #[test]
    fn restarted_peer_receives_after_long_session() {
        let mut b = Endpoint::bind(local()).unwrap();
        let mut a1 = Endpoint::bind(local()).unwrap();
        let addr = a1.local_addr();
        a1.send_reliable(b.local_addr(), b"hi", Instant::now());
        pump(&mut a1, &mut b, 1);
        // b sends far more than the reorder window to a1.
        for _ in 0..300 {
            b.send_reliable(addr, b"x", Instant::now());
        }
        assert_eq!(pump(&mut b, &mut a1, 300).len(), 300);
        drop(a1);

        let mut a2 = Endpoint::bind(addr).unwrap();
        a2.send_unreliable(b.local_addr(), b"back");
        pump(&mut a2, &mut b, 1);
        b.send_reliable(addr, b"welcome back", Instant::now());
        assert_eq!(
            pump(&mut b, &mut a2, 1),
            vec![(b"welcome back".to_vec(), true)]
        );
    }
}
