//! Cookie spoofing for forwarded X11 connections.
//!
//! We never hand the remote host the real cookie of the local display:
//! on a multi-user server anyone able to read the forwarded
//! `.Xauthority` would gain full access to the user's desktop (keystroke
//! capture, screen scraping). Instead a random FAKE cookie is minted per
//! session and announced in `x11-req`; every X11 channel the server
//! opens must then present that fake cookie, which we verify and swap
//! for the real one before the bytes reach the local X server. This is
//! what OpenSSH does, and it is why a mismatch is a hard reject.
//!
//! The swap happens in the X11 connection-setup request, the first thing
//! any X client sends:
//!
//! ```text
//! byte-order:1  unused:1  major:2  minor:2  n:2  d:2  unused:2
//! auth-protocol-name:n  pad(n)  auth-protocol-data:d  pad(d)
//! ```
//!
//! Two traps live in those 12 bytes: the length fields follow the
//! byte-order byte (NOT the host's endianness), and the setup request
//! can arrive split across several channel reads.

/// Length of an `MIT-MAGIC-COOKIE-1` cookie.
pub const COOKIE_LEN: usize = 16;
/// The only auth protocol we forward.
pub const COOKIE_PROTO: &str = "MIT-MAGIC-COOKIE-1";

/// Fixed-size prefix of the setup request.
const HEADER_LEN: usize = 12;
/// Guard against a malicious or desynchronized peer claiming huge
/// lengths; a real setup request is far under this.
const MAX_SETUP_LEN: usize = 8 * 1024;

const BYTE_ORDER_BIG: u8 = b'B';
const BYTE_ORDER_LITTLE: u8 = b'l';

/// Round `n` up to the next multiple of 4 (X11 pads every field).
fn pad4(n: usize) -> usize {
    (4 - (n % 4)) % 4
}

/// Constant-time byte comparison. Cookies are 16 bytes, so a
/// hand-rolled loop is fine; pulling in `subtle` for one helper is
/// overkill for an X11 auth cookie compare.
/// The length check short-circuits: a wrong LENGTH is not secret, only
/// the position of the first differing byte is.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Outcome of feeding bytes to [`SetupRewriter::push`].
#[derive(Debug, PartialEq, Eq)]
pub enum Rewrite {
    /// The setup request is incomplete; forward nothing yet.
    NeedMore,
    /// The setup request was verified and rewritten. `data` carries the
    /// rewritten setup request followed by any trailing bytes that were
    /// already buffered, ready to be written to the X server.
    Done(Vec<u8>),
    /// The peer presented something that is not the fake cookie we
    /// minted, or a malformed setup request. The channel must be closed.
    Reject(&'static str),
}

/// Accumulates the X11 setup request of a single forwarded channel and
/// swaps the announced fake cookie for the display's real one.
pub struct SetupRewriter {
    buf: Vec<u8>,
    fake: Vec<u8>,
    /// The local display's real cookie, or `None` when the display
    /// accepts unauthenticated clients (WSLg, VcXsrv `-ac`). `None`
    /// rewrites the setup request to carry NO auth at all, which is
    /// what OpenSSH does in the same situation.
    real: Option<Vec<u8>>,
    done: bool,
}

impl SetupRewriter {
    pub fn new(fake: Vec<u8>, real: Option<Vec<u8>>) -> Self {
        Self { buf: Vec::with_capacity(64), fake, real, done: false }
    }

    /// True once the setup request has been rewritten; further bytes on
    /// the channel are plain X protocol traffic and must be passed
    /// through untouched.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feed freshly-read channel bytes.
    pub fn push(&mut self, chunk: &[u8]) -> Rewrite {
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > MAX_SETUP_LEN {
            return Rewrite::Reject("X11 setup request exceeds sane size");
        }
        if self.buf.len() < HEADER_LEN {
            return Rewrite::NeedMore;
        }

        let big_endian = match self.buf[0] {
            BYTE_ORDER_BIG => true,
            BYTE_ORDER_LITTLE => false,
            _ => return Rewrite::Reject("X11 setup request has an invalid byte-order byte"),
        };
        let u16_at = |b: &[u8], i: usize| -> usize {
            let (a, c) = (b[i] as usize, b[i + 1] as usize);
            if big_endian { (a << 8) | c } else { (c << 8) | a }
        };

        let name_len = u16_at(&self.buf, 6);
        let data_len = u16_at(&self.buf, 8);
        let name_at = HEADER_LEN;
        let data_at = name_at + name_len + pad4(name_len);
        let total = data_at + data_len + pad4(data_len);
        if total > MAX_SETUP_LEN {
            return Rewrite::Reject("X11 setup request declares an implausible length");
        }
        if self.buf.len() < total {
            return Rewrite::NeedMore;
        }

        let name = &self.buf[name_at..name_at + name_len];
        let data = &self.buf[data_at..data_at + data_len];

        if name != COOKIE_PROTO.as_bytes() {
            // Includes the unauthenticated case (n == 0): the server was
            // told to use a cookie, so an empty auth means the channel is
            // not the one we authorized.
            return Rewrite::Reject("X11 client offered an unexpected auth protocol");
        }
        // Constant-time, like OpenSSH's `timingsafe_bcmp` at the same
        // point (`channels.c`, `x11_open_helper`). A mismatch kills the
        // channel, but NOT the guessing: every X11 channel the server
        // opens gets a fresh rewriter, and it can open them without
        // limit, so a byte-at-a-time `memcmp` would leak the cookie to
        // anyone with a foothold on the remote host.
        if !constant_time_eq(data, &self.fake) {
            return Rewrite::Reject("X11 cookie mismatch");
        }

        // What the LOCAL display expects, which is not what the remote
        // sent: a display with no access control gets a setup request
        // with the auth stripped entirely.
        let (out_name, out_data): (&[u8], &[u8]) = match &self.real {
            Some(real) => (COOKIE_PROTO.as_bytes(), real.as_slice()),
            None => (b"", b""),
        };

        // Rebuild rather than patch in place: the replacement auth
        // routinely differs in length from what arrived.
        let mut out = Vec::with_capacity(total + (self.buf.len() - total));
        out.extend_from_slice(&self.buf[..6]);
        let put_u16 = |out: &mut Vec<u8>, v: usize| {
            let v = v as u16;
            if big_endian {
                out.extend_from_slice(&v.to_be_bytes());
            } else {
                out.extend_from_slice(&v.to_le_bytes());
            }
        };
        put_u16(&mut out, out_name.len());
        put_u16(&mut out, out_data.len());
        out.extend_from_slice(&self.buf[10..12]);
        out.extend_from_slice(out_name);
        out.extend(std::iter::repeat_n(0u8, pad4(out_name.len())));
        out.extend_from_slice(out_data);
        out.extend(std::iter::repeat_n(0u8, pad4(out_data.len())));
        // Anything already buffered past the setup request is ordinary
        // traffic and must not be dropped.
        out.extend_from_slice(&self.buf[total..]);

        self.done = true;
        self.buf = Vec::new();
        Rewrite::Done(out)
    }
}

/// Mint a fresh 16-byte fake cookie.
pub fn random_cookie() -> Vec<u8> {
    let mut buf = vec![0u8; COOKIE_LEN];
    getrandom::fill(&mut buf).expect("OS RNG unavailable");
    buf
}

/// Lower-case hex, the form `x11-req` carries on the wire.
///
/// russh writes the cookie string verbatim into the packet, so the hex
/// encoding is OUR job; sending raw bytes here makes the server write
/// garbage into the remote `.Xauthority` and every X client fails to
/// authenticate with no useful diagnostic.
pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a setup request carrying `cookie`.
    fn setup(big_endian: bool, proto: &[u8], cookie: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(if big_endian { BYTE_ORDER_BIG } else { BYTE_ORDER_LITTLE });
        v.push(0);
        let put = |v: &mut Vec<u8>, n: u16| {
            if big_endian {
                v.extend_from_slice(&n.to_be_bytes());
            } else {
                v.extend_from_slice(&n.to_le_bytes());
            }
        };
        put(&mut v, 11); // major
        put(&mut v, 0); // minor
        put(&mut v, proto.len() as u16);
        put(&mut v, cookie.len() as u16);
        v.extend_from_slice(&[0, 0]); // unused
        v.extend_from_slice(proto);
        v.extend(std::iter::repeat_n(0u8, pad4(proto.len())));
        v.extend_from_slice(cookie);
        v.extend(std::iter::repeat_n(0u8, pad4(cookie.len())));
        v
    }

    fn fake() -> Vec<u8> {
        vec![0xAA; COOKIE_LEN]
    }
    fn real() -> Vec<u8> {
        vec![0xBB; COOKIE_LEN]
    }

    /// Extract the cookie back out of a rewritten packet.
    fn cookie_of(packet: &[u8], big_endian: bool) -> Vec<u8> {
        let u16_at = |i: usize| -> usize {
            let (a, c) = (packet[i] as usize, packet[i + 1] as usize);
            if big_endian { (a << 8) | c } else { (c << 8) | a }
        };
        let n = u16_at(6);
        let d = u16_at(8);
        let at = HEADER_LEN + n + pad4(n);
        packet[at..at + d].to_vec()
    }

    #[test]
    fn swaps_fake_for_real_little_endian() {
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        let Rewrite::Done(out) = rw.push(&setup(false, COOKIE_PROTO.as_bytes(), &fake())) else {
            panic!("expected a rewrite");
        };
        assert_eq!(cookie_of(&out, false), real());
        assert!(rw.is_done());
    }

    /// The length fields follow the packet's OWN byte-order byte. A
    /// host-endian read would mis-size the fields on this input.
    #[test]
    fn swaps_fake_for_real_big_endian() {
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        let Rewrite::Done(out) = rw.push(&setup(true, COOKIE_PROTO.as_bytes(), &fake())) else {
            panic!("expected a rewrite");
        };
        assert_eq!(cookie_of(&out, true), real());
    }

    /// A setup request split across reads must not be forwarded early.
    #[test]
    fn reassembles_a_fragmented_setup_request() {
        let packet = setup(false, COOKIE_PROTO.as_bytes(), &fake());
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        for byte in &packet[..packet.len() - 1] {
            assert_eq!(rw.push(&[*byte]), Rewrite::NeedMore);
        }
        let Rewrite::Done(out) = rw.push(&packet[packet.len() - 1..]) else {
            panic!("expected a rewrite once the last byte arrived");
        };
        assert_eq!(cookie_of(&out, false), real());
    }

    /// Bytes arriving in the same read after the setup request are real
    /// X traffic; dropping them corrupts the stream.
    #[test]
    fn preserves_trailing_traffic() {
        let mut packet = setup(false, COOKIE_PROTO.as_bytes(), &fake());
        packet.extend_from_slice(b"TRAILING");
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        let Rewrite::Done(out) = rw.push(&packet) else { panic!("expected a rewrite") };
        assert!(out.ends_with(b"TRAILING"));
    }

    #[test]
    fn rejects_a_wrong_cookie() {
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        let bogus = vec![0x11; COOKIE_LEN];
        assert!(matches!(
            rw.push(&setup(false, COOKIE_PROTO.as_bytes(), &bogus)),
            Rewrite::Reject(_)
        ));
    }

    /// An unauthenticated client on an authorized channel is a reject,
    /// not a pass-through: we told the server to require a cookie.
    #[test]
    fn rejects_an_empty_auth() {
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        assert!(matches!(rw.push(&setup(false, b"", &[])), Rewrite::Reject(_)));
    }

    #[test]
    fn rejects_a_bad_byte_order_byte() {
        let mut packet = setup(false, COOKIE_PROTO.as_bytes(), &fake());
        packet[0] = b'Z';
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        assert!(matches!(rw.push(&packet), Rewrite::Reject(_)));
    }

    #[test]
    fn rejects_an_unknown_auth_protocol() {
        let mut rw = SetupRewriter::new(fake(), Some(real()));
        assert!(matches!(
            rw.push(&setup(false, b"XDM-AUTHORIZATION-1", &fake())),
            Rewrite::Reject(_)
        ));
    }

    /// The unauthenticated-display case (WSLg, VcXsrv `-ac`): the remote
    /// still presents the fake cookie, and the rewrite must STRIP the
    /// auth rather than substitute one, because the local display would
    /// reject a cookie it never issued.
    #[test]
    fn strips_auth_for_an_open_display() {
        let mut rw = SetupRewriter::new(fake(), None);
        let Rewrite::Done(out) = rw.push(&setup(false, COOKIE_PROTO.as_bytes(), &fake())) else {
            panic!("expected a rewrite");
        };
        let u16_at = |i: usize| -> usize {
            (out[i + 1] as usize) << 8 | out[i] as usize
        };
        assert_eq!(u16_at(6), 0, "auth-protocol-name must be emptied");
        assert_eq!(u16_at(8), 0, "auth-protocol-data must be emptied");
        assert_eq!(out.len(), HEADER_LEN, "no name/data/padding should remain");
    }

    /// Even with the auth stripped, traffic that arrived in the same read
    /// must survive the rebuild.
    #[test]
    fn open_display_rewrite_preserves_trailing_traffic() {
        let mut packet = setup(true, COOKIE_PROTO.as_bytes(), &fake());
        packet.extend_from_slice(b"AFTER");
        let mut rw = SetupRewriter::new(fake(), None);
        let Rewrite::Done(out) = rw.push(&packet) else { panic!("expected a rewrite") };
        assert!(out.ends_with(b"AFTER"));
        assert_eq!(out.len(), HEADER_LEN + 5);
    }

    #[test]
    fn hex_encoding_is_lowercase_and_padded() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
        assert_eq!(to_hex(&random_cookie()).len(), COOKIE_LEN * 2);
    }

    #[test]
    fn random_cookies_differ() {
        assert_ne!(random_cookie(), random_cookie());
    }

    /// The cookie compare must agree with `==` on every shape that
    /// reaches it: equal, differing in the first byte, differing in the
    /// last (the case a short-circuiting compare would time-leak), and
    /// length mismatches in both directions.
    #[test]
    fn constant_time_eq_matches_plain_equality() {
        let base = vec![0xAA; COOKIE_LEN];
        assert!(constant_time_eq(&base, &base.clone()));

        let mut first_differs = base.clone();
        first_differs[0] ^= 0xFF;
        assert!(!constant_time_eq(&base, &first_differs));

        let mut last_differs = base.clone();
        last_differs[COOKIE_LEN - 1] ^= 0x01;
        assert!(!constant_time_eq(&base, &last_differs));

        assert!(!constant_time_eq(&base, &base[..COOKIE_LEN - 1]));
        assert!(!constant_time_eq(&base[..COOKIE_LEN - 1], &base));
        assert!(constant_time_eq(&[], &[]));
    }
}
