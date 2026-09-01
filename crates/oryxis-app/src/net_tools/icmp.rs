//! ICMP echo without privileges, which is what makes ping and traceroute
//! work on a machine that has no `ping` or `traceroute` installed.
//!
//! A raw socket needs root everywhere, so this uses the unprivileged path
//! each platform actually provides, and says so honestly when there is
//! none:
//!
//! - **Linux**: a datagram ICMP socket (`SOCK_DGRAM` + `IPPROTO_ICMP`),
//!   the one `ping` itself uses when it is not setuid. The kernel owns
//!   the echo id and matches replies to the socket. Whether it is allowed
//!   is the `net.ipv4.ping_group_range` sysctl, so a refusal is an
//!   ordinary value here, not an error: the caller falls back to the
//!   system binary.
//! - **Windows**: `IcmpSendEcho2`, the documented API for exactly this,
//!   which needs no elevation. IPv6 would be `Icmp6SendEcho2`; it is
//!   deliberately not wired, so a v6 target falls back to `ping`/
//!   `tracert`, which ship with the OS.
//! - **macOS and the BSDs**: nothing portable. Datagram ICMP exists on
//!   macOS but its behaviour for the ICMP ERRORS a traceroute is built
//!   on is undocumented, and it cannot be verified from here, so the
//!   system binaries are used instead. They are part of the base system
//!   on those platforms, which is exactly why this is a fair trade
//!   there and was not on Linux.
//!
//! Packet building and reading are pure functions over bytes, tested
//! without a network; the unsafe is confined to the send / receive.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

// The packet vocabulary belongs to the platforms that build echo
// requests themselves. Windows does not: `IcmpSendEcho2` takes a payload
// and owns the header, so everything below it is dead code there and the
// allow says so rather than a cfg fence that the tests would have to
// climb too.
/// ICMPv4 message types this cares about.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
const ICMPV4_ECHO_REQUEST: u8 = 8;
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
const ICMPV4_ECHO_REPLY: u8 = 0;
/// ICMPv6 numbers them differently, and the two sets overlap in value,
/// which is why nothing here matches on the number alone.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
const ICMPV6_ECHO_REQUEST: u8 = 128;
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
const ICMPV6_ECHO_REPLY: u8 = 129;

/// Why the native path is not available. The caller reads this to decide
/// between falling back quietly and telling the user something.
///
/// Each platform constructs a subset (Linux cannot report `Platform`,
/// Windows cannot report `Denied`), and the whole enum is still the
/// contract every one of them answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Unavailable {
    /// This platform has no unprivileged path (macOS, the BSDs), or the
    /// address family is not wired natively.
    Platform,
    /// The OS refused: `ping_group_range` on Linux, most often.
    Denied,
    /// Something else went wrong while setting the probe up.
    Failed(String),
}

/// What one probe got back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// The target itself answered: an echo reply.
    Reply { from: IpAddr, rtt_ms: f32 },
    /// A router on the way answered: TTL exceeded in transit.
    Hop { from: IpAddr, rtt_ms: f32 },
    /// Someone reported the target cannot be reached.
    Unreachable { from: IpAddr, rtt_ms: f32 },
    /// Nothing came back inside the budget.
    Timeout,
}

impl Outcome {
    /// Whether the probe reached the address it was aimed at, which is
    /// what ends a traceroute.
    pub(crate) fn is_final(&self) -> bool {
        matches!(self, Outcome::Reply { .. } | Outcome::Unreachable { .. })
    }

    pub(crate) fn source(&self) -> Option<IpAddr> {
        match self {
            Outcome::Reply { from, .. }
            | Outcome::Hop { from, .. }
            | Outcome::Unreachable { from, .. } => Some(*from),
            Outcome::Timeout => None,
        }
    }

    pub(crate) fn rtt_ms(&self) -> Option<f32> {
        match self {
            Outcome::Reply { rtt_ms, .. }
            | Outcome::Hop { rtt_ms, .. }
            | Outcome::Unreachable { rtt_ms, .. } => Some(*rtt_ms),
            Outcome::Timeout => None,
        }
    }
}

/// The internet checksum (RFC 1071): one's complement of the one's
/// complement sum of 16-bit words. Used for ICMPv4; the kernel computes
/// the ICMPv6 one itself, because that checksum covers a pseudo-header
/// only the stack knows.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn checksum(bytes: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let (words, remainder) = bytes.as_chunks::<2>();
    for word in words {
        sum += u32::from(u16::from_be_bytes(*word));
    }
    if let [last] = remainder {
        // An odd trailing byte is the high half of a padded word, not a
        // byte to drop.
        sum += u32::from(u16::from_be_bytes([*last, 0]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// An echo request: header plus a payload the reply echoes back.
///
/// `id` is written even though a datagram socket has the kernel
/// overwrite it, so the same builder serves both platforms and the
/// packet is well-formed wherever it is read.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn echo_request(v6: bool, id: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
    let kind = if v6 { ICMPV6_ECHO_REQUEST } else { ICMPV4_ECHO_REQUEST };
    let mut packet = Vec::with_capacity(8 + payload.len());
    packet.push(kind);
    packet.push(0); // code
    packet.extend_from_slice(&[0, 0]); // checksum, filled below
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&seq.to_be_bytes());
    packet.extend_from_slice(payload);
    if !v6 {
        let sum = checksum(&packet);
        packet[2..4].copy_from_slice(&sum.to_be_bytes());
    }
    packet
}

/// The sequence number of an echo REPLY, or `None` when the bytes are
/// something else. The id is not checked: a datagram ICMP socket only
/// ever receives replies to its own requests (the kernel rewrites and
/// matches the id), so the sequence is what pairs a reply with a probe.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) fn echo_reply_seq(v6: bool, bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 8 {
        return None;
    }
    let expected = if v6 { ICMPV6_ECHO_REPLY } else { ICMPV4_ECHO_REPLY };
    if bytes[0] != expected {
        return None;
    }
    Some(u16::from_be_bytes([bytes[6], bytes[7]]))
}

/// Send `count` echo requests and report each one. `Err` means the
/// native path is unavailable and the caller should fall back; a probe
/// that went out and got nothing back is `Outcome::Timeout`, which is an
/// answer.
pub(crate) fn ping(
    ip: IpAddr,
    count: u8,
    timeout: Duration,
) -> Result<Vec<Outcome>, Unavailable> {
    let mut out = Vec::with_capacity(count as usize);
    for seq in 0..count {
        // The default TTL: this is a reachability question, not a path
        // one, so nothing is limiting how far the packet may travel.
        out.push(probe(ip, 64, seq as u16, timeout)?);
        // Ping's own pacing. Without it four probes leave in a burst,
        // which measures the queue rather than the path.
        if seq + 1 < count {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    Ok(out)
}

/// Walk the path one TTL at a time, stopping at the target or at
/// `max_hops`. Each entry is one hop, in order.
pub(crate) fn traceroute(
    ip: IpAddr,
    max_hops: u8,
    timeout: Duration,
) -> Result<Vec<Outcome>, Unavailable> {
    let mut hops = Vec::new();
    for ttl in 1..=max_hops {
        let outcome = probe(ip, ttl, u16::from(ttl), timeout)?;
        let done = outcome.is_final();
        hops.push(outcome);
        if done {
            break;
        }
    }
    Ok(hops)
}

/// One probe at one TTL. Blocking on purpose: the caller runs the whole
/// walk on a blocking task, because the error-queue read this depends on
/// signals through `POLLERR`, and threading that through an async
/// readiness layer buys nothing for a probe that lasts a second.
#[cfg(target_os = "linux")]
fn probe(ip: IpAddr, ttl: u8, seq: u16, timeout: Duration) -> Result<Outcome, Unavailable> {
    linux::probe(ip, ttl, seq, timeout)
}

#[cfg(target_os = "windows")]
fn probe(ip: IpAddr, ttl: u8, seq: u16, timeout: Duration) -> Result<Outcome, Unavailable> {
    let _ = seq;
    match ip {
        IpAddr::V4(v4) => windows_icmp::probe(v4, ttl, timeout),
        // Icmp6SendEcho2 is a different call with a different reply
        // shape; until it is wired, a v6 target is the system binary's
        // job (both ship with Windows).
        IpAddr::V6(_) => Err(Unavailable::Platform),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn probe(_ip: IpAddr, _ttl: u8, _seq: u16, _timeout: Duration) -> Result<Outcome, Unavailable> {
    Err(Unavailable::Platform)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::net::SocketAddr;
    use std::os::fd::AsRawFd;

    use socket2::{Domain, Protocol, SockAddr, Socket, Type};

    pub(super) fn probe(
        ip: IpAddr,
        ttl: u8,
        seq: u16,
        timeout: Duration,
    ) -> Result<Outcome, Unavailable> {
        let v6 = ip.is_ipv6();
        let (domain, protocol) = if v6 {
            (Domain::IPV6, Protocol::ICMPV6)
        } else {
            (Domain::IPV4, Protocol::ICMPV4)
        };
        let socket = Socket::new(domain, Type::DGRAM, Some(protocol)).map_err(|e| {
            // EACCES / EPERM is the sysctl saying no, which is the
            // ordinary case on a locked-down kernel and the whole reason
            // the fallback exists.
            match e.raw_os_error() {
                Some(libc::EACCES) | Some(libc::EPERM) => Unavailable::Denied,
                _ => Unavailable::Failed(e.to_string()),
            }
        })?;
        // The ICMP error that reports an expired TTL is delivered on the
        // socket's ERROR QUEUE, not as a datagram, so without this the
        // hop would simply time out and every traceroute would be a
        // column of stars.
        enable_recverr(&socket, v6)?;
        if v6 {
            socket.set_unicast_hops_v6(u32::from(ttl))
        } else {
            socket.set_ttl_v4(u32::from(ttl))
        }
        .map_err(|e| Unavailable::Failed(e.to_string()))?;
        socket
            .set_read_timeout(Some(timeout))
            .map_err(|e| Unavailable::Failed(e.to_string()))?;

        let packet = echo_request(v6, std::process::id() as u16, seq, b"oryxis-net-tools");
        let dest = SockAddr::from(SocketAddr::new(ip, 0));
        let sent = std::time::Instant::now();
        if let Err(e) = socket.send_to(&packet, &dest) {
            // The packet never left, so there is nothing to time out on.
            // A missing route (ENETUNREACH) is a LOCAL fact, and
            // reporting it as silence would blame the network for this
            // machine's configuration; handing it back as unavailable
            // sends the caller to the system binary, which prints the
            // real reason.
            return match e.raw_os_error() {
                Some(libc::EACCES) | Some(libc::EPERM) => Err(Unavailable::Denied),
                _ => Err(Unavailable::Failed(e.to_string())),
            };
        }

        let deadline = sent + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Ok(Outcome::Timeout);
            }
            let events = poll_once(socket.as_raw_fd(), left);
            if events == 0 {
                return Ok(Outcome::Timeout);
            }
            let rtt = || sent.elapsed().as_secs_f32() * 1000.0;
            // The error queue first: a TTL expiry arrives there while
            // POLLIN may also be set by an unrelated late reply.
            if events & libc::POLLERR != 0
                && let Some(outcome) = read_error_queue(&socket, v6, rtt())
            {
                return Ok(outcome);
            }
            if events & libc::POLLIN != 0 {
                let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 1500];
                match socket.recv(&mut buf) {
                    Ok(n) => {
                        // SAFETY: `recv` reported `n` initialized bytes.
                        let bytes =
                            unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), n) };
                        if echo_reply_seq(v6, bytes) == Some(seq) {
                            return Ok(Outcome::Reply { from: ip, rtt_ms: rtt() });
                        }
                        // Someone else's reply on a shared socket is not
                        // possible here, but a stale one from a previous
                        // probe is: keep waiting rather than reporting
                        // the wrong round trip.
                        continue;
                    }
                    Err(_) => return Ok(Outcome::Timeout),
                }
            }
        }
    }

    /// Ask the kernel to queue ICMP errors for this socket.
    fn enable_recverr(socket: &Socket, v6: bool) -> Result<(), Unavailable> {
        let (level, name) = if v6 {
            (libc::IPPROTO_IPV6, libc::IPV6_RECVERR)
        } else {
            (libc::IPPROTO_IP, libc::IP_RECVERR)
        };
        let on: libc::c_int = 1;
        // SAFETY: the fd is owned by `socket` and outlives the call; the
        // option value is a single `c_int` and its length is passed.
        let rc = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                level,
                name,
                std::ptr::addr_of!(on).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(Unavailable::Failed(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        Ok(())
    }

    /// Wait for readability or an error, returning the revents mask (0 on
    /// timeout).
    fn poll_once(fd: libc::c_int, budget: Duration) -> libc::c_short {
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ms = budget.as_millis().min(i32::MAX as u128) as libc::c_int;
        // SAFETY: one initialized pollfd is passed with a count of one.
        let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
        if rc <= 0 { 0 } else { pfd.revents }
    }

    /// Read one queued ICMP error and turn it into an outcome. `None`
    /// when the queue held something this does not describe, so the
    /// caller keeps waiting instead of reporting a hop that never
    /// answered.
    fn read_error_queue(socket: &Socket, v6: bool, rtt_ms: f32) -> Option<Outcome> {
        let mut buf = [0u8; 512];
        let mut control = [0u8; 512];
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        // SAFETY: msghdr is plain data; every pointer below refers to a
        // local buffer that outlives the call.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = control.len() as _;

        // SAFETY: the fd is owned by `socket`; `msg` and its buffers are
        // initialized above and live across the call.
        let n = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut msg, libc::MSG_ERRQUEUE) };
        if n < 0 {
            return None;
        }
        let want_level = if v6 { libc::IPPROTO_IPV6 } else { libc::IPPROTO_IP };
        let want_type = if v6 { libc::IPV6_RECVERR } else { libc::IP_RECVERR };
        // SAFETY: walking the control buffer with the kernel's own
        // macros, which is the documented way to read cmsgs.
        let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
        while !cmsg.is_null() {
            // SAFETY: CMSG_FIRSTHDR / CMSG_NXTHDR only ever return a
            // pointer to a header inside the control buffer.
            let header = unsafe { &*cmsg };
            if header.cmsg_level == want_level && header.cmsg_type == want_type {
                // SAFETY: for IP_RECVERR the payload starts with a
                // `sock_extended_err`, followed by the offender address.
                let err = unsafe { &*(libc::CMSG_DATA(cmsg).cast::<libc::sock_extended_err>()) };
                let origin_ok = err.ee_origin == libc::SO_EE_ORIGIN_ICMP
                    || err.ee_origin == libc::SO_EE_ORIGIN_ICMP6;
                if origin_ok {
                    // SAFETY: SO_EE_OFFENDER is the address that follows
                    // the struct in the same payload.
                    let offender = unsafe {
                        libc::CMSG_DATA(cmsg).add(std::mem::size_of::<libc::sock_extended_err>())
                    };
                    let from = read_offender(offender, v6);
                    return Some(classify(err.ee_type, err.ee_code, v6, from, rtt_ms));
                }
            }
            // SAFETY: same contract as CMSG_FIRSTHDR.
            cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
        }
        None
    }

    /// The router that reported the error, read out of the offender
    /// sockaddr the kernel appended.
    fn read_offender(ptr: *const u8, v6: bool) -> Option<IpAddr> {
        if ptr.is_null() {
            return None;
        }
        if v6 {
            // SAFETY: the offender for an ICMPv6 error is a
            // `sockaddr_in6`, which the kernel wrote in full.
            let addr = unsafe { &*(ptr.cast::<libc::sockaddr_in6>()) };
            if addr.sin6_family != libc::AF_INET6 as libc::sa_family_t {
                return None;
            }
            Some(IpAddr::V6(std::net::Ipv6Addr::from(addr.sin6_addr.s6_addr)))
        } else {
            // SAFETY: and a `sockaddr_in` for ICMPv4.
            let addr = unsafe { &*(ptr.cast::<libc::sockaddr_in>()) };
            if addr.sin_family != libc::AF_INET as libc::sa_family_t {
                return None;
            }
            Some(IpAddr::V4(Ipv4Addr::from(u32::from_be(
                addr.sin_addr.s_addr,
            ))))
        }
    }

    /// Which outcome an ICMP error type means. The v4 and v6 numbers
    /// differ, which is why the family decides before the number does.
    fn classify(
        ee_type: u8,
        _ee_code: u8,
        v6: bool,
        from: Option<IpAddr>,
        rtt_ms: f32,
    ) -> Outcome {
        let Some(from) = from else {
            return Outcome::Timeout;
        };
        let time_exceeded = if v6 { 3 } else { 11 };
        let unreachable = if v6 { 1 } else { 3 };
        if ee_type == time_exceeded {
            Outcome::Hop { from, rtt_ms }
        } else if ee_type == unreachable {
            Outcome::Unreachable { from, rtt_ms }
        } else {
            // Something else answered (a parameter problem, a quench):
            // it still proves a machine at that address is on the path.
            Outcome::Hop { from, rtt_ms }
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_icmp {
    use super::*;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        IcmpCreateFile, IcmpSendEcho2, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
    };

    /// Reply statuses that matter. `IP_SUCCESS` is the target itself;
    /// the rest are documented `IP_STATUS` values from ipexport.h.
    const IP_SUCCESS: u32 = 0;
    const IP_DEST_NET_UNREACHABLE: u32 = 11002;
    const IP_DEST_HOST_UNREACHABLE: u32 = 11003;
    const IP_DEST_PROT_UNREACHABLE: u32 = 11004;
    const IP_DEST_PORT_UNREACHABLE: u32 = 11005;
    const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;

    pub(super) fn probe(
        ip: Ipv4Addr,
        ttl: u8,
        timeout: Duration,
    ) -> Result<Outcome, Unavailable> {
        // SAFETY: no arguments; returns INVALID_HANDLE_VALUE on failure,
        // which is checked before the handle is used.
        let handle: HANDLE = unsafe { IcmpCreateFile() };
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err(Unavailable::Failed(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let guard = HandleGuard(handle);
        let payload = *b"oryxis-net-tools";
        let options = IP_OPTION_INFORMATION {
            Ttl: ttl,
            Tos: 0,
            Flags: 0,
            OptionsSize: 0,
            OptionsData: std::ptr::null_mut(),
        };
        // The documented sizing: one reply plus the payload plus room
        // for the error message the API may append.
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 64];
        let timeout_ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;
        // SAFETY: the handle is valid (checked above), the buffers are
        // owned locally and their sizes are passed alongside them, and
        // the call is synchronous because event and APC are null.
        let count = unsafe {
            IcmpSendEcho2(
                guard.0,
                std::ptr::null_mut(),
                None,
                std::ptr::null_mut(),
                u32::from_ne_bytes(ip.octets()),
                payload.as_ptr().cast(),
                payload.len() as u16,
                &options,
                reply.as_mut_ptr().cast(),
                reply.len() as u32,
                timeout_ms,
            )
        };
        if count == 0 {
            // Zero replies is the ordinary "nothing came back" answer;
            // the API reports the reason through GetLastError, and every
            // one of them means the same thing to the card.
            return Ok(Outcome::Timeout);
        }
        // SAFETY: the API reported at least one reply written into the
        // buffer, which is laid out as an `ICMP_ECHO_REPLY`.
        let echo = unsafe { &*(reply.as_ptr().cast::<ICMP_ECHO_REPLY>()) };
        let from = IpAddr::V4(Ipv4Addr::from(echo.Address.to_ne_bytes()));
        let rtt_ms = echo.RoundTripTime as f32;
        Ok(match echo.Status {
            IP_SUCCESS => Outcome::Reply { from, rtt_ms },
            IP_TTL_EXPIRED_TRANSIT => Outcome::Hop { from, rtt_ms },
            IP_DEST_NET_UNREACHABLE
            | IP_DEST_HOST_UNREACHABLE
            | IP_DEST_PROT_UNREACHABLE
            | IP_DEST_PORT_UNREACHABLE => Outcome::Unreachable { from, rtt_ms },
            _ => Outcome::Timeout,
        })
    }

    /// Closes the ICMP handle on every path out, including the early
    /// returns above.
    struct HandleGuard(HANDLE);

    impl Drop for HandleGuard {
        fn drop(&mut self) {
            // SAFETY: the handle came from IcmpCreateFile and is closed
            // exactly once, here.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_rfc1071_worked_example() {
        // The RFC's own example octets and their sum.
        let bytes = [0x00u8, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        assert_eq!(checksum(&bytes), 0x220d);
    }

    #[test]
    fn checksum_of_a_packet_including_it_is_zero() {
        // The receiver's check: summing a correct packet gives zero.
        let packet = echo_request(false, 0x1234, 7, b"payload");
        assert_eq!(checksum(&packet), 0);
    }

    #[test]
    fn checksum_handles_an_odd_length() {
        // The trailing byte is padded, not dropped.
        assert_ne!(checksum(&[0x01, 0x02, 0x03]), checksum(&[0x01, 0x02]));
    }

    #[test]
    fn echo_request_is_well_formed() {
        let packet = echo_request(false, 0xabcd, 3, b"hi");
        assert_eq!(packet[0], ICMPV4_ECHO_REQUEST);
        assert_eq!(packet[1], 0, "code");
        assert_eq!(u16::from_be_bytes([packet[4], packet[5]]), 0xabcd, "id");
        assert_eq!(u16::from_be_bytes([packet[6], packet[7]]), 3, "seq");
        assert_eq!(&packet[8..], b"hi");
    }

    #[test]
    fn ipv6_request_leaves_the_checksum_to_the_kernel() {
        // The ICMPv6 checksum covers a pseudo-header built from the
        // addresses the stack picks, so a value computed here would be
        // wrong as often as not.
        let packet = echo_request(true, 1, 1, b"x");
        assert_eq!(packet[0], ICMPV6_ECHO_REQUEST);
        assert_eq!(&packet[2..4], &[0, 0]);
    }

    #[test]
    fn a_reply_is_paired_by_sequence() {
        let mut reply = echo_request(false, 9, 42, b"x");
        reply[0] = ICMPV4_ECHO_REPLY;
        assert_eq!(echo_reply_seq(false, &reply), Some(42));
        // A request is not a reply, whatever else it carries.
        assert_eq!(echo_reply_seq(false, &echo_request(false, 9, 42, b"x")), None);
        // And the families do not answer for each other, even though
        // their type numbers overlap.
        assert_eq!(echo_reply_seq(true, &reply), None);
    }

    #[test]
    fn truncated_bytes_are_not_a_reply() {
        assert_eq!(echo_reply_seq(false, &[]), None);
        assert_eq!(echo_reply_seq(false, &[0, 0, 0]), None);
    }

    /// The native path against loopback, which is the only target that
    /// answers with no network at all.
    ///
    /// Skipped, not failed, where the OS refuses the socket: that is the
    /// ordinary state of a locked-down kernel (`ping_group_range`) and of
    /// every platform with no wired native path, and it is exactly what
    /// the fallback exists for. Verified for real under
    /// `unshare -rn` + `sysctl -w net.ipv4.ping_group_range="0 0"`,
    /// which is how to run it on a machine that denies it by default.
    #[test]
    fn native_ping_answers_from_loopback() {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let outcomes = match ping(ip, 2, Duration::from_secs(2)) {
            Ok(outcomes) => outcomes,
            Err(Unavailable::Denied | Unavailable::Platform) => return,
            Err(Unavailable::Failed(e)) => panic!("native ping setup failed: {e}"),
        };
        assert_eq!(outcomes.len(), 2, "one outcome per request");
        for outcome in &outcomes {
            assert!(
                matches!(outcome, Outcome::Reply { from, .. } if *from == ip),
                "loopback must answer its own echo: {outcome:?}"
            );
        }
    }

    #[test]
    fn native_traceroute_to_loopback_is_one_hop() {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let hops = match traceroute(ip, 5, Duration::from_secs(2)) {
            Ok(hops) => hops,
            Err(Unavailable::Denied | Unavailable::Platform) => return,
            Err(Unavailable::Failed(e)) => panic!("native traceroute setup failed: {e}"),
        };
        // The walk stops at the target, so a local address is exactly one
        // entry: a second would mean `is_final` failed to end it.
        assert_eq!(hops.len(), 1, "loopback is one hop: {hops:?}");
        assert!(hops[0].is_final());
        assert_eq!(hops[0].source(), Some(ip));
    }

    #[test]
    fn only_a_reached_target_ends_a_traceroute() {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert!(Outcome::Reply { from: ip, rtt_ms: 1.0 }.is_final());
        assert!(Outcome::Unreachable { from: ip, rtt_ms: 1.0 }.is_final());
        // A hop and a silence are the middle of the walk, not its end.
        assert!(!Outcome::Hop { from: ip, rtt_ms: 1.0 }.is_final());
        assert!(!Outcome::Timeout.is_final());
    }
}
