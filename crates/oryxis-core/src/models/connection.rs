use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::cloud::CloudRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: Uuid,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    /// Wire protocol for this host. Defaults to SSH so every payload
    /// written before the field existed (old vaults, sync peers,
    /// portable exports) keeps meaning exactly what it meant. Telnet
    /// hosts reuse the same encrypted password column and ride sync /
    /// export unchanged; the editor swaps to a reduced form and the
    /// terminal pane picks the matching transport at connect.
    #[serde(default)]
    pub protocol: ConnectionProtocol,
    /// Serial-line parameters, meaningful only when `protocol` is
    /// `Serial` (the port path itself reuses `hostname`). `None` on
    /// every non-serial host and on legacy payloads; the connect path
    /// falls back to `SerialParams::default()` (9600 8N1).
    #[serde(default)]
    pub serial: Option<super::serial::SerialParams>,
    /// Telnet-only options (TLS and its per-host verification escape),
    /// meaningful only when `protocol` is `Telnet`. `None` on every
    /// other host and on legacy payloads, which is plain Telnet with
    /// verification on: an old payload can never decode into a session
    /// that skips certificate checks.
    #[serde(default)]
    pub telnet: Option<super::telnet::TelnetOptions>,
    /// mosh options, meaningful only when `protocol` is `Ssh`. `None`
    /// is an ordinary SSH shell, which is what every payload written
    /// before this existed carries. mosh rides ON an SSH host rather
    /// than replacing one, because the server is started over SSH and
    /// answers over it; see `models::mosh`.
    #[serde(default)]
    pub mosh: Option<super::mosh::MoshOptions>,
    /// Local-shell settings (which curated terminal to spawn and where
    /// it starts), meaningful only when `protocol` is `Local`. `None`
    /// falls back to the user's default shell in its own default
    /// directory, the same session the local-terminal picker opens.
    #[serde(default)]
    pub local: Option<super::local::LocalConfig>,
    /// Remote-desktop kind (RDP vs VNC). Meaningful only when `protocol`
    /// is `RemoteDesktop`; ignored otherwise. `#[serde(default)]` -> RDP
    /// on legacy payloads.
    #[serde(default)]
    pub rd_kind: super::remote_desktop::RemoteDesktopKind,
    /// Optional SSH host to tunnel the remote-desktop connection through.
    /// `Some(id)` routes through that connection's SSH session (the
    /// launcher opens an ephemeral `-L` forward to `hostname:port`);
    /// `None` connects the desktop endpoint directly. A dangling id
    /// resolves to direct with a warning, never an error (mirrors
    /// `proxy_identity_id`). Meaningful only for `RemoteDesktop`.
    #[serde(default)]
    pub rd_gateway_id: Option<Uuid>,
    /// Outbound address-family preference for this host's dials
    /// (PuTTY's Auto / IPv4 / IPv6). Applies to the direct dial, the
    /// proxy dial and the first jump hop, everything that opens a real
    /// socket from this machine. `#[serde(default)]` -> Auto on legacy
    /// payloads, and sync / portable export ride the serde field.
    #[serde(default)]
    pub address_family: AddressFamily,
    /// MAC address for Wake-on-LAN, stored canonical
    /// ("AA:BB:CC:DD:EE:FF", normalized at editor save). `None` hides
    /// the card's Wake on LAN action. `#[serde(default)]` -> None on
    /// legacy payloads; sync / portable export ride the serde field.
    #[serde(default)]
    pub mac_address: Option<String>,
    pub username: Option<String>,
    pub auth_method: AuthMethod,
    pub key_id: Option<Uuid>,
    /// Offer a key read from the user's `~/.ssh` directory when this
    /// host resolves no vault key. Off by default and per-host on
    /// purpose (same shape as `x11_forwarding`): offering a credential
    /// the user never named changes who they are to THAT server, so it
    /// is never a global switch. Only consulted for auth methods that
    /// use a key at all (`Key` / `Auto` / `Certificate`); an identity
    /// or a linked `key_id` always wins, the disk is strictly the gap
    /// filler. `#[serde(default)]` -> false on legacy payloads, and
    /// sync / portable export ride the serde field.
    #[serde(default)]
    pub use_disk_key: bool,
    /// Which file `use_disk_key` reads, as an absolute path or a
    /// `~/`-prefixed one (OpenSSH's `IdentityFile`, and what the
    /// ssh_config importer writes here). `None` scans the default
    /// OpenSSH names in `~/.ssh` instead. Meaningless while
    /// `use_disk_key` is false. A PATH, never key material: the file
    /// stays on disk and is read at connect time, so this column is
    /// plaintext like `notes`.
    #[serde(default)]
    pub identity_file: Option<String>,
    pub identity_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub jump_chain: Vec<Uuid>,
    pub proxy: Option<ProxyConfig>,
    /// Reference to a saved `ProxyIdentity`. When set, takes precedence
    /// over the inline `proxy` field, the SSH engine resolves the
    /// identity (via the vault) and ignores `proxy`. `None` falls back
    /// to inline. Cleared on cascade when the identity is deleted.
    #[serde(default)]
    pub proxy_identity_id: Option<Uuid>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    /// Accent color for the host. Persisted as a hex string ("#RRGGBB").
    /// Used by the dynamic accent system to tint the chrome / tab
    /// indicator when this host is active, and as the fill / border
    /// color for the host icon. `None` falls back to the global accent.
    pub color: Option<String>,
    #[serde(default)]
    pub port_forwards: Vec<PortForward>,
    /// Environment variables sent to the remote shell via SSH `setenv`
    /// before the shell starts. Note most `sshd` only accept `LC_*` /
    /// `LANG_*` unless `AcceptEnv` is widened.
    #[serde(default)]
    pub env_vars: Vec<EnvVar>,
    /// Per-host character encoding label (e.g. `"Big5"`). `None` = UTF-8.
    /// Drives PTY transcoding in the SSH engine for legacy charsets.
    #[serde(default)]
    pub encoding: Option<String>,
    /// How wide this host's terminal draws Unicode "Ambiguous" width
    /// characters. See [`AmbiguousWidth`].
    #[serde(default)]
    pub ambiguous_width: AmbiguousWidth,
    /// Per-host terminal type sent to the server as `TERM` when requesting the
    /// PTY (e.g. `"xterm"`, `"linux"`, `"vt100"`). `None` = `xterm-256color`.
    /// Lets the user pick a fallback for hosts whose terminfo trips on the
    /// default (older boxes, some `mc` / curses setups).
    #[serde(default)]
    pub terminal_type: Option<String>,
    pub mcp_enabled: bool,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Detected remote OS id, populated the first time we successfully SSH
    /// into this host and the OS-detection setting is enabled. Values are
    /// lowercase `ID=` from `/etc/os-release` for Linux (ubuntu / debian /
    /// alpine / rhel / fedora / arch / amzn / centos / rocky / alma / suse)
    /// or `uname -s` lowercased for non-Linux (darwin / freebsd / openbsd /
    /// netbsd). `None` means unknown, show the generic server icon.
    #[serde(default)]
    pub detected_os: Option<String>,
    /// User-chosen icon id (overrides the auto-detected one). When present,
    /// the OS-detection probe is skipped and the stored icon / color are
    /// used verbatim on host cards / tabs / editor.
    #[serde(default)]
    pub custom_icon: Option<String>,
    /// User-chosen icon-background color as a hex string (e.g. `#E95420`).
    /// Paired with `custom_icon`.
    #[serde(default)]
    pub custom_color: Option<String>,
    /// Forward the local ssh-agent socket to the remote shell. When
    /// enabled, after the session channel is open we send an
    /// `auth-agent-req@openssh.com` request; sshd then sets
    /// `SSH_AUTH_SOCK` on the remote side and tunnels back any reads
    /// from that socket through this SSH transport. Lets the user
    /// `ssh hostB` from inside hostA without staging keys remotely.
    #[serde(default)]
    pub agent_forwarding: bool,
    /// Forward X11 (OpenSSH's `ForwardX11` / `-Y`). When enabled we send
    /// an `x11-req` before the shell starts; sshd then exports `DISPLAY`
    /// on the remote and opens an X11 channel back to us for every GUI
    /// app launched there, which we bridge to the local X server.
    ///
    /// Trusted mode: untrusted forwarding relies on the X SECURITY
    /// extension, which denies the pointer/keyboard grabs Java toolkits
    /// need, so enterprise GUIs fail to start under it.
    ///
    /// `#[serde(default)]` so payloads written before this field (sync
    /// peers, older exports) still deserialize.
    #[serde(default)]
    pub x11_forwarding: bool,
    /// Opt-in agentless resource monitoring for this host (issue #83):
    /// the terminal sidebar's Monitor tab polls `/proc` over the live
    /// session. Off by default, unlike `mcp_enabled`: monitoring costs a
    /// recurring probe, so it is never enabled behind the user's back.
    /// `#[serde(default)]` so payloads written before this field (sync
    /// peers, older exports) still deserialize.
    #[serde(default)]
    pub monitor_enabled: bool,
    /// Which mounts the monitor reports for this host (issue #135).
    /// `None` is Auto: the probe's own rules keep one row per storage
    /// device and drop pseudo filesystems and bind mounts, which is
    /// right on nearly every host. `Some(list)` is Custom: ONLY the
    /// mounts matching those patterns are reported, for the hosts whose
    /// mount table no rule can guess (an Android phone, a container
    /// farm, a NAS with 40 pool members where two matter).
    ///
    /// A pattern is a mount path, exact unless it contains `*`, which
    /// matches any run of characters (`/mnt/*`). `Some(vec![])` is a
    /// deliberate answer, not an empty value: it is "report no disks on
    /// this host", the same shape as `HostHighlightRules::replace` with
    /// an empty list.
    #[serde(default)]
    pub monitor_disks: Option<Vec<String>>,
    /// Per-host terminal palette override. When set, takes precedence
    /// over the global `terminal_theme_override` setting and the app
    /// theme fallback. Stored as `TerminalTheme::name()` (e.g.
    /// "Dracula", "Monokai") so the value survives palette additions
    /// without a migration. `None` falls through to the global pick.
    #[serde(default)]
    pub terminal_theme: Option<String>,
    /// Set on hosts imported from a cloud profile (EC2 in v0.6). Carries
    /// the stable resource handle so the connect path can re-resolve
    /// hostname / pick the right transport on each session. `None` for
    /// manually-added hosts.
    #[serde(default)]
    pub cloud_ref: Option<CloudRef>,
    /// Sent to the remote shell right after the session opens. Used to
    /// escape minimal entry shells (`exec bash` on ECS / distroless) or
    /// to drop into a specific working directory. `None` skips the step.
    #[serde(default)]
    pub initial_command: Option<String>,
    /// When set, the startup command is resolved live from this snippet's
    /// body at connect time (so editing the snippet updates every host
    /// that references it). Takes precedence over `initial_command`; a
    /// dangling id (snippet deleted) resolves to no command, never an
    /// error. `None` means the startup command is the literal
    /// `initial_command` (custom) or nothing.
    #[serde(default)]
    pub startup_snippet_id: Option<Uuid>,
    /// Expect/send automation replayed after the SSH login, for hosts
    /// that authenticate INSIDE the TTY (JumpServer-class bastions,
    /// menu-driven jump boxes). Points at a shared `LoginScript`; a
    /// dangling id (script deleted) resolves to no automation, never an
    /// error, same rule as `proxy_identity_id`.
    #[serde(default)]
    pub login_script_id: Option<Uuid>,
    /// Values for the script's `{placeholder}` variables on THIS host,
    /// which is what lets one script serve many assets behind the same
    /// bastion. Plaintext by construction: a credential can never be a
    /// variable, it is a `SecretRef` in the script and an encrypted
    /// column here.
    #[serde(default)]
    pub login_script_vars: Vec<ScriptVar>,
    /// Per-host SSH keepalive override (seconds). `None` inherits the
    /// global `keepalive_interval` setting. `Some(0)` explicitly disables
    /// keepalive on this host even when the global default is non-zero.
    /// `Some(n)` overrides the global with `n` seconds.
    #[serde(default)]
    pub keepalive_interval: Option<u32>,
    /// Per-host override for showing the shell-set window title (OSC 0/2) in
    /// the tab strip. `None` inherits the global `terminal_auto_title`
    /// setting; `Some(true)` always shows the shell title for this host,
    /// `Some(false)` always keeps this host's curated label.
    #[serde(default)]
    pub auto_title: Option<bool>,
    /// Shape to use when rendering this host's icon in cards / tabs /
    /// sidebar. Valid values: `"circular"`, `"square"`, `"outline"`,
    /// `"initials"`. `None` falls back to the global
    /// `default_host_icon` setting (default `"circular"` in v0.7).
    /// Stored as a String to keep the wire / sync payload identical for
    /// older peers that never saw the field.
    #[serde(default)]
    pub icon_style: Option<String>,
    /// Names of fields the user has explicitly overridden after this
    /// host was imported from a cloud provider. Reimport / refresh
    /// flows consult this list before overwriting a field with the
    /// upstream value: if the field name appears here, the user's value
    /// wins. Empty on manually-added hosts and on freshly-imported
    /// cloud hosts. Today only `label`, `hostname`, and `username` are
    /// tracked since those are the fields AWS discovery actually
    /// pushes; the structure stays open-ended so future providers can
    /// flag more without a schema change.
    #[serde(default)]
    pub customized_fields: Vec<String>,
    /// Per-host override for terminal session recording. `None` follows
    /// the global `session_logging` setting; `Some(true)` always records
    /// this host (even when the global toggle is off); `Some(false)`
    /// never records it (even when the global toggle is on).
    #[serde(default)]
    pub session_logging: Option<bool>,
    /// Per-host SSH algorithm overrides, one list per negotiation
    /// category. `None` = `Auto` (russh's safe defaults, untouched);
    /// `Some(list)` pins exactly those algorithm names (in order) for the
    /// category, which is how a user reaches legacy servers that only
    /// offer cbc / sha1 / dh-group1. Names are the on-the-wire strings
    /// (e.g. `"aes256-cbc"`, `"diffie-hellman-group14-sha1"`,
    /// `"hmac-sha1"`, `"ssh-rsa"`); unknown names are ignored by the
    /// engine. Stored as plain strings so the sync / export payload stays
    /// identical for older peers that never saw the fields.
    #[serde(default)]
    pub ciphers: Option<Vec<String>>,
    #[serde(default)]
    pub kex: Option<Vec<String>>,
    #[serde(default)]
    pub macs: Option<Vec<String>>,
    #[serde(default)]
    pub host_key_algorithms: Option<Vec<String>>,
    /// Per-host override for Privacy Mode (auto-hide sensitive data:
    /// host / ip / user / port / proxy on cards and logs, plus IP and
    /// `user@host` prompt tokens in the terminal). `None` follows the
    /// global `privacy_mode` setting; `Some(true)` always hides for this
    /// host (even when the global toggle is off); `Some(false)` never
    /// hides it (even when the global toggle is on).
    #[serde(default)]
    pub privacy_mode: Option<bool>,
    /// Per-host override for auto-opening the terminal sidebar when a
    /// session to this host opens. `None` follows the global
    /// `sidebar_auto_open` setting; `Some(true)` always opens it for
    /// this host; `Some(false)` never opens it automatically.
    #[serde(default)]
    pub sidebar_auto_open: Option<bool>,
    /// Per-host legacy keyboard modes + terminal feature toggles (C5:
    /// backspace / home-end / function-key encoding, mouse-reporting /
    /// title-change / OSC 52 gates). `None` = all defaults (today's xterm
    /// behaviour), which keeps old payloads byte-identical. Resolve via
    /// [`super::terminal_quirks::TerminalQuirks`]; a `None` here means
    /// [`super::terminal_quirks::DEFAULT_QUIRKS`].
    #[serde(default)]
    pub quirks: Option<super::terminal_quirks::TerminalQuirks>,
    /// Per-host terminal backdrop overrides (opacity, background
    /// picture). `None` = inherit every global setting, which is what an
    /// untouched host carries and what keeps old payloads unchanged.
    /// Resolve via [`super::terminal_appearance::TerminalAppearance`].
    #[serde(default)]
    pub terminal_appearance: Option<super::terminal_appearance::TerminalAppearance>,
    /// This host's own highlight rules, and whether they add to the
    /// global list or replace it (C6). `None` = the host has none and
    /// follows the global list, which is what every existing payload
    /// carries. Its own column rather than a field of
    /// `terminal_appearance`: that one holds four small `Option`s about
    /// one picture, and a list of rules inside it would make both
    /// unreadable.
    #[serde(default)]
    pub highlight_rules: Option<super::highlight_rule::HostHighlightRules>,
    /// Per-host SSH rekey threshold in megabytes (`None` = russh default).
    /// A plain additive column; rides sync / export like the algorithm
    /// overrides above.
    #[serde(default)]
    pub rekey_limit_mb: Option<u32>,
    /// Directory a fresh SFTP mount of this host lands in. `None` (and the
    /// empty string) means the login directory, the previous behaviour. A
    /// path that no longer resolves falls back to the login directory
    /// rather than failing the mount, so a stale value never locks the
    /// host out of its own file browser.
    #[serde(default)]
    pub sftp_initial_path: Option<String>,
    /// OS drag-and-drop onto this host's terminal uploads over ZMODEM
    /// (`rz` typed into the shell) instead of SFTP. For hosts whose
    /// interactive shell runs INSIDE a container (the startup command
    /// enters one: docker exec, a containerised sshd's shell): SFTP
    /// always reaches the host filesystem as sshd sees it, while the
    /// `rz` the app types runs where the shell runs and writes into the
    /// container's own working directory. Off (the default) keeps the
    /// standard drop routing: SFTP when the shell's cwd is exactly
    /// known, ZMODEM otherwise.
    #[serde(default)]
    pub zmodem_drops: bool,
}

impl Connection {
    /// Drop the fields that are a LOCAL trust decision rather than host
    /// data, for a copy about to leave this machine (the sync wire, a
    /// portable export).
    ///
    /// Today that is exactly one field: `telnet.tls_insecure`, which
    /// says "accept a certificate the trust store rejects". Replicating
    /// the DATA is right, and every other Telnet field travels; letting
    /// this one travel would disarm certificate verification on a
    /// machine whose owner never saw the appliance it was turned on for.
    /// Same rule as `trusted_proxy_commands`, which is local-only by
    /// construction for the same reason.
    ///
    /// Applied on SEND, so an old peer that still transmits the flag
    /// cannot arm it here either: `apply_records` runs this too.
    pub fn strip_local_trust(&mut self) {
        if let Some(telnet) = self.telnet.as_mut() {
            telnet.tls_insecure = false;
            // An options blob that now says nothing is no blob at all,
            // which keeps a plain-Telnet host byte-identical on both
            // sides of a sync.
            if telnet.is_default() {
                self.telnet = None;
            }
        }
    }

    /// Whether this host's terminal draws ambiguous-width characters two
    /// cells wide, with `Auto` resolved against its encoding.
    pub fn ambiguous_width_effective(&self) -> bool {
        self.ambiguous_width.resolve(self.encoding.as_deref())
    }

    pub fn new(label: impl Into<String>, hostname: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            hostname: hostname.into(),
            port: 22,
            protocol: ConnectionProtocol::Ssh,
            serial: None,
            telnet: None,
            mosh: None,
            local: None,
            rd_kind: super::remote_desktop::RemoteDesktopKind::default(),
            rd_gateway_id: None,
            address_family: AddressFamily::default(),
            username: None,
            auth_method: AuthMethod::Auto,
            key_id: None,
            use_disk_key: false,
            identity_file: None,
            identity_id: None,
            group_id: None,
            jump_chain: Vec::new(),
            port_forwards: Vec::new(),
            env_vars: Vec::new(),
            encoding: None,
            ambiguous_width: AmbiguousWidth::default(),
            terminal_type: None,
            proxy: None,
            proxy_identity_id: None,
            tags: Vec::new(),
            notes: None,
            color: None,
            mcp_enabled: true,
            last_used: None,
            created_at: now,
            updated_at: now,
            detected_os: None,
            custom_icon: None,
            custom_color: None,
            agent_forwarding: false,
            x11_forwarding: false,
            monitor_enabled: false,
            monitor_disks: None,
            terminal_theme: None,
            cloud_ref: None,
            initial_command: None,
            startup_snippet_id: None,
            login_script_id: None,
            login_script_vars: Vec::new(),
            keepalive_interval: None,
            mac_address: None,
            auto_title: None,
            icon_style: None,
            customized_fields: Vec::new(),
            session_logging: None,
            ciphers: None,
            kex: None,
            macs: None,
            host_key_algorithms: None,
            privacy_mode: None,
            sidebar_auto_open: None,
            quirks: None,
            terminal_appearance: None,
            highlight_rules: None,
            rekey_limit_mb: None,
            sftp_initial_path: None,
            zmodem_drops: false,
        }
    }
}

/// Which wire protocol a connection speaks. One selector per host, not
/// a per-host stack of protocols: the whole `Connection` model is
/// single-endpoint, so a host that needs both SSH and Telnet is two
/// hosts. Serialized as a plain string variant so older peers that
/// never saw the field simply ignore it on receive and omit it on send
/// (covered by the legacy-payload test below).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ConnectionProtocol {
    #[default]
    Ssh,
    Telnet,
    /// A bare TCP socket: bytes in, bytes out, no option negotiation
    /// and no NVT line-ending rules (PuTTY calls this "Raw"). What
    /// console / terminal servers expose their serial ports on, so a
    /// switch reached through a Lantronix or Opengear box is the same
    /// session a null-modem cable would give. `hostname`/`port` are the
    /// endpoint; the protocol carries no credentials of its own, so the
    /// editor hides them.
    Raw,
    /// Local serial line (no network). The port path lives in
    /// `hostname`; line parameters live in `Connection.serial`.
    Serial,
    /// A shell on THIS machine, saved like any other host so it can
    /// carry a folder, a startup command, environment variables, a
    /// theme and a group. Which shell to spawn is a reference into the
    /// curated local-terminal list (`Connection.local`), never a copy
    /// of its program path.
    Local,
    /// Remote desktop (RDP/VNC). Unlike the others this is NOT a terminal
    /// transport: it opens no pane. `hostname`/`port` are the desktop
    /// endpoint and `username`/`password` its login; `rd_kind` picks
    /// RDP vs VNC and `rd_gateway_id` optionally routes the connection
    /// through an SSH host (the tunnel). The connect action launches the
    /// OS-native client instead of opening a terminal.
    RemoteDesktop,
}

impl ConnectionProtocol {
    /// Conventional TCP port, used by the host editor to swap the
    /// numeric-port default when the picker changes (22 <-> 23).
    /// `RemoteDesktop` reports the RDP default (3389); the kind picker
    /// refines it to VNC's 5900.
    ///
    /// `None` means "no port to suggest", which happens for two
    /// different reasons: `Serial` and `Local` have no network port at
    /// all (see [`uses_network_port`](Self::uses_network_port)), while
    /// `Raw` has one that is required and unguessable, console servers
    /// map each serial line to its own port (2001, 3001, 7001, ...) and
    /// no vendor agrees. Suggesting one there would look authoritative
    /// and be wrong, so the field stays as the user left it.
    pub fn default_port(self) -> Option<u16> {
        match self {
            ConnectionProtocol::Ssh => Some(22),
            ConnectionProtocol::Telnet => Some(23),
            ConnectionProtocol::Raw => None,
            ConnectionProtocol::Serial => None,
            ConnectionProtocol::Local => None,
            ConnectionProtocol::RemoteDesktop => Some(3389),
        }
    }

    /// Whether `port` is a port nobody typed on purpose: the
    /// conventional number of one of the protocols this app speaks
    /// (22 SSH, 23 Telnet, 992 telnets, 3389 RDP, 5900 VNC).
    ///
    /// The host editor uses it to decide whether switching protocols
    /// may retarget the field. Comparing against the PREVIOUS
    /// protocol's default alone is not enough: a hop through Serial or
    /// Local (neither has a port) breaks the chain, and the field then
    /// keeps a 22 into a Telnet host. A user-typed 2222 still survives,
    /// which is what the rule is protecting.
    pub fn is_conventional_port(port: u16) -> bool {
        matches!(port, 22 | 23 | 992 | 3389 | 5900)
    }

    /// Whether this protocol dials a TCP endpoint, i.e. whether the
    /// host editor shows the numeric port field at all. Distinct from
    /// [`default_port`](Self::default_port), which answers "what should
    /// it start at": `Raw` needs the field and has no default.
    pub fn uses_network_port(self) -> bool {
        !matches!(self, ConnectionProtocol::Serial | ConnectionProtocol::Local)
    }

    /// Whether this protocol drives a terminal pane. `RemoteDesktop`
    /// does not, it launches an external client, so the terminal /
    /// SFTP / MCP paths must exclude it.
    pub fn is_terminal(self) -> bool {
        !matches!(self, ConnectionProtocol::RemoteDesktop)
    }

    /// Whether this protocol reaches another machine over the network.
    /// `Serial` runs down a cable and `Local` never leaves this box, so
    /// neither takes a hostname, a proxy, a jump chain or a Wake-on-LAN
    /// packet.
    pub fn is_remote(self) -> bool {
        !matches!(self, ConnectionProtocol::Serial | ConnectionProtocol::Local)
    }

    /// Whether the protocol authenticates with a username / password of
    /// its own. Raw is a bare socket and Serial a cable (both let the
    /// device do its own prompting, in band); Local runs as the user
    /// already logged in.
    pub fn uses_credentials(self) -> bool {
        !matches!(
            self,
            ConnectionProtocol::Raw | ConnectionProtocol::Serial | ConnectionProtocol::Local
        )
    }

    /// The `scheme://` this protocol is written as in quick connect.
    /// `Local` has none: it names no endpoint, so there is nothing for
    /// an ad-hoc target to say.
    pub fn scheme(self) -> Option<&'static str> {
        match self {
            ConnectionProtocol::Ssh => Some("ssh"),
            ConnectionProtocol::Telnet => Some("telnet"),
            ConnectionProtocol::Raw => Some("raw"),
            ConnectionProtocol::Serial => Some("serial"),
            ConnectionProtocol::RemoteDesktop => Some("rdp"),
            ConnectionProtocol::Local => None,
        }
    }

    /// Parse a quick-connect scheme back into a protocol, case
    /// insensitively. `telnets` is the conventional name for Telnet
    /// over TLS (port 992) and resolves to `Telnet`; the caller turns
    /// on the TLS option.
    pub fn from_scheme(scheme: &str) -> Option<Self> {
        match scheme.to_ascii_lowercase().as_str() {
            "ssh" => Some(ConnectionProtocol::Ssh),
            "telnet" | "telnets" => Some(ConnectionProtocol::Telnet),
            "raw" | "tcp" => Some(ConnectionProtocol::Raw),
            "serial" => Some(ConnectionProtocol::Serial),
            "rdp" | "vnc" => Some(ConnectionProtocol::RemoteDesktop),
            _ => None,
        }
    }
}

// Display feeds the host editor's pick_list mapper directly (the fork's
// 4-step pick_list API renders via `|p| p.to_string()`).
impl std::fmt::Display for ConnectionProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionProtocol::Ssh => write!(f, "SSH"),
            ConnectionProtocol::Telnet => write!(f, "Telnet"),
            ConnectionProtocol::Raw => write!(f, "Raw"),
            ConnectionProtocol::Serial => write!(f, "Serial"),
            ConnectionProtocol::Local => write!(f, "Local"),
            ConnectionProtocol::RemoteDesktop => write!(f, "Remote Desktop"),
        }
    }
}

/// Address-family preference for outbound dials (PuTTY's Auto / IPv4 /
/// IPv6 setting). `Auto` takes the resolver's order; `V4` / `V6` keep
/// only that family's addresses and fail honestly when the name
/// resolves to none of them.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AddressFamily {
    #[default]
    Auto,
    V4,
    V6,
}

// Display feeds the host editor's pick_list mapper.
impl std::fmt::Display for AddressFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddressFamily::Auto => write!(f, "Auto"),
            AddressFamily::V4 => write!(f, "IPv4"),
            AddressFamily::V6 => write!(f, "IPv6"),
        }
    }
}

/// How many cells a character in the Unicode East Asian Width class
/// "Ambiguous" occupies on this host.
///
/// The class (box drawing U+2500..U+257F, circled digits, arrows, Greek
/// and Cyrillic letters, `±`, `·`) is one cell wide in Western contexts
/// and two in legacy CJK ones. Width is a two-party contract: this side
/// decides how to DRAW, the remote's `wcwidth` decides where programs
/// PUT things, and misaligned vim borders or htop bars are what a
/// disagreement looks like. So this is per host, following that host's
/// locale, rather than a global truth.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AmbiguousWidth {
    /// Follow this host's encoding: a CJK charset means the remote is a
    /// legacy CJK environment, everything else stays narrow.
    #[default]
    Auto,
    Narrow,
    Wide,
}

impl AmbiguousWidth {
    /// Whether ambiguous characters take two cells, given the host's
    /// encoding label.
    ///
    /// `Auto` reads the label through `encoding_rs` rather than matching
    /// strings, so the aliases a synced or imported host may carry
    /// (`csbig5`, `x-gbk`, `ms_kanji`) resolve like the canonical names.
    pub fn resolve(self, encoding: Option<&str>) -> bool {
        match self {
            AmbiguousWidth::Wide => true,
            AmbiguousWidth::Narrow => false,
            AmbiguousWidth::Auto => encoding
                .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
                .is_some_and(|enc| {
                    enc == encoding_rs::BIG5
                        || enc == encoding_rs::GBK
                        || enc == encoding_rs::GB18030
                        || enc == encoding_rs::EUC_KR
                        || enc == encoding_rs::SHIFT_JIS
                        || enc == encoding_rs::EUC_JP
                        || enc == encoding_rs::ISO_2022_JP
                }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AuthMethod {
    #[default]
    Auto,
    Password,
    Key,
    Agent,
    Interactive,
    /// Password auth where the password is never stored: the app prompts
    /// for it at every connect and feeds the typed value straight to the
    /// server. Falls back to any caller-provided password when no UI
    /// prompt channel is wired (headless / MCP).
    PasswordPrompt,
    /// Certificate-only publickey auth (B2.1): the selected key's attached
    /// OpenSSH user certificate is offered and nothing else. No bare-key
    /// fallback, no password fallback; an unusable or missing certificate
    /// is a hard auth error. `Key` is the strict opposite (bare key only)
    /// and `Auto` remains the smart path (cert, then bare key, then the
    /// other methods).
    Certificate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortForward {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

/// One `{placeholder}` value for this host's login script. A named
/// pair rather than a tuple so the serialized form stays readable and
/// can gain fields (a per-variable "secret" flag is deliberately NOT
/// one of them: secrets are `SecretRef`s, never variables).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptVar {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub proxy_type: ProxyType,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    /// Proxy password. Hydrated in-memory by the vault
    /// (`get_proxy_password`) right before connect. Marked `serde(skip)`
    /// so it never lands in the `proxy` column (which is plaintext JSON)
    ///, the credential lives in the encrypted `proxy_password` column.
    #[serde(skip)]
    pub password: Option<String>,
}

// Hand-written so a hydrated proxy formatted with `{:?}` (logs, error
// chains) can never print the password.
impl std::fmt::Debug for ProxyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyConfig")
            .field("proxy_type", &self.proxy_type)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProxyType {
    Socks5,
    Socks4,
    Http,
    Command(String),
}

/// Stable identifier for one command-proxy line, used as the key of the
/// per-device approval a `ProxyType::Command` needs before it may be
/// spawned (`VaultStore::is_proxy_command_trusted`).
///
/// The hash is taken over the line AS STORED, tokens and all, which is
/// also the form the approval prompt shows. Editing a single character
/// mints a different fingerprint and the approval has to be given again,
/// which is what stops a trusted line from being quietly rewritten into
/// another one.
///
/// `%h` / `%n` / `%p` / `%r` are resolved after the gate, not before, so one
/// approval covers every host that shares the proxy rather than
/// re-prompting per target. That splits the identity of what runs in
/// two: the line is pinned here, and the values allowed into its token
/// slots are constrained where the substitution happens
/// (`oryxis-ssh`'s `proxy_spawn`), to shapes that cannot restructure it.
///
/// Hashed rather than stored in the clear because the line is
/// user-authored and can embed credentials (the connect log already
/// prints only its TYPE for that reason).
pub fn proxy_command_fingerprint(command: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(command.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fingerprint keys a local approval to run a process, so it
    /// has to be exact: two lines that differ at all are two decisions,
    /// including by whitespace, which is enough to turn one command
    /// into another.
    #[test]
    fn proxy_command_fingerprint_is_exact() {
        let a = proxy_command_fingerprint("ssh -W %h:%p bastion");
        assert_eq!(a.len(), 64, "SHA-256 in hex");
        assert_eq!(a, proxy_command_fingerprint("ssh -W %h:%p bastion"));
        assert_ne!(a, proxy_command_fingerprint("ssh -W %h:%p bastion "));
        assert_ne!(a, proxy_command_fingerprint("ssh -W %h:%p bastionx"));
        assert_ne!(a, proxy_command_fingerprint(""));
    }

    #[test]
    fn new_connection_defaults() {
        let conn = Connection::new("test", "10.0.0.1");
        assert_eq!(conn.label, "test");
        assert_eq!(conn.hostname, "10.0.0.1");
        assert_eq!(conn.port, 22);
        assert_eq!(conn.auth_method, AuthMethod::Auto);
        assert!(conn.username.is_none());
        assert!(conn.jump_chain.is_empty());
        assert!(conn.proxy.is_none());
    }

    #[test]
    fn connection_serialization_roundtrip() {
        let mut conn = Connection::new("prod", "server.example.com");
        conn.username = Some("deploy".into());
        conn.auth_method = AuthMethod::Key;
        conn.tags = vec!["production".into(), "web".into()];

        let json = serde_json::to_string(&conn).unwrap();
        let deserialized: Connection = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.label, "prod");
        assert_eq!(deserialized.hostname, "server.example.com");
        assert_eq!(deserialized.username, Some("deploy".into()));
        assert_eq!(deserialized.auth_method, AuthMethod::Key);
        assert_eq!(deserialized.tags.len(), 2);
    }

    #[test]
    fn proxy_config_serialization() {
        let proxy = ProxyConfig {
            proxy_type: ProxyType::Socks5,
            host: "proxy.local".into(),
            port: 1080,
            username: Some("user".into()),
            password: None,
        };

        let json = serde_json::to_string(&proxy).unwrap();
        let de: ProxyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(de.proxy_type, ProxyType::Socks5);
        assert_eq!(de.port, 1080);
        assert_eq!(de.username.as_deref(), Some("user"));
        assert!(de.password.is_none());
    }

    /// `password` is `serde(skip)`, it must not appear in serialized
    /// JSON nor be read back. This guards against credential leaks via
    /// the plaintext `proxy` column.
    #[test]
    fn proxy_config_password_is_not_serialized() {
        let proxy = ProxyConfig {
            proxy_type: ProxyType::Http,
            host: "proxy.local".into(),
            port: 8080,
            username: Some("u".into()),
            password: Some("topsecret".into()),
        };

        let json = serde_json::to_string(&proxy).unwrap();
        assert!(
            !json.contains("topsecret"),
            "password leaked into ProxyConfig JSON: {json}"
        );
        assert!(
            !json.contains("password"),
            "password key should not appear at all: {json}"
        );

        let de: ProxyConfig = serde_json::from_str(&json).unwrap();
        assert!(de.password.is_none());
    }

    /// Legacy peers (sync wire) and old portable exports never carried
    /// the `keepalive_interval` field. Receiving such a payload must
    /// deserialize cleanly with the field defaulting to `None` (= inherit
    /// global). Without `#[serde(default)]` on the field, this would
    /// regress the moment a v1 peer talks to a v2 peer.
    #[test]
    fn keepalive_interval_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        // Simulate a payload from a peer that never knew about the field.
        value.as_object_mut().unwrap().remove("keepalive_interval");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.keepalive_interval, None);
    }

    /// A peer or export that predates the setting carries no field, and
    /// must land on `Auto`: the narrow default is what every host had.
    #[test]
    fn ambiguous_width_legacy_payload_defaults_to_auto() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("ambiguous_width");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.ambiguous_width, AmbiguousWidth::Auto);
        assert!(!de.ambiguous_width_effective());
    }

    #[test]
    fn ambiguous_width_explicit_choices_ignore_the_encoding() {
        assert!(AmbiguousWidth::Wide.resolve(None));
        assert!(AmbiguousWidth::Wide.resolve(Some("UTF-8")));
        assert!(!AmbiguousWidth::Narrow.resolve(Some("Big5")));
        assert!(!AmbiguousWidth::Narrow.resolve(Some("GBK")));
    }

    #[test]
    fn ambiguous_width_auto_follows_the_encoding() {
        // A legacy CJK charset is the only per-host signal we have that
        // the remote is a wide-ambiguous environment.
        for label in ["Big5", "GBK", "gb18030", "EUC-KR", "Shift_JIS", "EUC-JP", "ISO-2022-JP"] {
            assert!(
                AmbiguousWidth::Auto.resolve(Some(label)),
                "{label} should resolve wide",
            );
        }
        // Aliases resolve like the canonical names, which is the whole
        // reason the label goes through encoding_rs.
        for label in ["csbig5", "x-gbk", "ms_kanji", "csEUCKR"] {
            assert!(
                AmbiguousWidth::Auto.resolve(Some(label)),
                "{label} should resolve wide",
            );
        }
        for label in [None, Some("UTF-8"), Some("ISO-8859-1"), Some("windows-1251")] {
            assert!(!AmbiguousWidth::Auto.resolve(label), "{label:?} should resolve narrow");
        }
        // An unreadable label is not a CJK host.
        assert!(!AmbiguousWidth::Auto.resolve(Some("not-an-encoding")));
    }

    /// A peer or export that predates the disk key source carries
    /// neither key, and must land with the source OFF: the whole point
    /// of the opt-in is that no host offers a credential the user never
    /// named, and a sync payload is not consent either.
    #[test]
    fn disk_key_legacy_payload_leaves_the_source_off() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("use_disk_key");
        obj.remove("identity_file");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert!(!de.use_disk_key);
        assert_eq!(de.identity_file, None);
    }

    /// Both fields ride sync and portable export on the serde flatten,
    /// so a round trip has to keep them: the path is plain data, and
    /// dropping it would move which key the host offers.
    #[test]
    fn disk_key_fields_survive_a_serde_round_trip() {
        let mut conn = Connection::new("h", "10.0.0.1");
        conn.use_disk_key = true;
        conn.identity_file = Some("~/.ssh/work_ed25519".into());
        let de: Connection =
            serde_json::from_value(serde_json::to_value(&conn).unwrap()).unwrap();
        assert!(de.use_disk_key);
        assert_eq!(de.identity_file.as_deref(), Some("~/.ssh/work_ed25519"));
    }

    /// A peer or export that predates per-host highlight rules carries
    /// no `highlight_rules` key, and must land as `None` = "follow the
    /// global list", not as an empty override that would silently mean
    /// something else once `replace` exists.
    #[test]
    fn highlight_rules_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("highlight_rules");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.highlight_rules, None);
    }

    /// Same contract for the Wake-on-LAN MAC: a payload written before
    /// the field existed carries no `mac_address` key and must land as
    /// `None`, which hides the card action for that host.
    #[test]
    fn mac_address_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("mac_address");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.mac_address, None);
    }

    /// Same contract for the login script: a payload from a peer that
    /// never knew about it carries neither key, and both must land as
    /// "no automation". Defaulting `login_script_vars` to an empty list
    /// matters as much as the id: a script resolved with no variables
    /// would type literal `{asset}` text at a bastion prompt.
    #[test]
    fn login_script_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("login_script_id");
        obj.remove("login_script_vars");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.login_script_id, None);
        assert!(de.login_script_vars.is_empty());
    }

    /// Same contract for the monitoring opt-in: a payload written before
    /// host monitoring existed carries no `monitor_enabled` key and must
    /// land as `false`. Defaulting the other way would silently start
    /// probing every host a legacy peer syncs over.
    #[test]
    fn monitor_enabled_legacy_payload_defaults_to_false() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("monitor_enabled");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert!(!de.monitor_enabled);
    }

    /// The disk selection is three-state, so the legacy payload must
    /// land on Auto (`None`) rather than on an empty Custom list, which
    /// would report no disks at all on every host a legacy peer syncs.
    #[test]
    fn monitor_disks_legacy_payload_defaults_to_auto() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("monitor_disks");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.monitor_disks, None);

        // And an explicitly empty list survives the round trip as
        // itself: "report no disks here" is an answer, not a missing
        // value.
        let mut conn = Connection::new("quiet", "10.0.0.2");
        conn.monitor_disks = Some(Vec::new());
        let de: Connection =
            serde_json::from_value(serde_json::to_value(&conn).unwrap()).unwrap();
        assert_eq!(de.monitor_disks, Some(Vec::new()));
    }

    /// Same contract for X11 forwarding: a sync peer or export written
    /// before the field existed carries no `x11_forwarding` key, and it
    /// must land OFF. Defaulting it on would silently start exposing
    /// the local display to every previously-saved host.
    #[test]
    fn x11_forwarding_legacy_payload_defaults_to_false() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("x11_forwarding");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert!(!de.x11_forwarding);
    }

    /// Same contract for the protocol selector: a payload written
    /// before Telnet existed carries no `protocol` key and must land as
    /// `Ssh`, because that is what every pre-existing host is.
    #[test]
    fn protocol_legacy_payload_defaults_to_ssh() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("protocol");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.protocol, ConnectionProtocol::Ssh);
    }

    /// Same contract for the address-family preference: a payload from
    /// before the field existed must land as `Auto` (the behavior every
    /// pre-existing host had).
    #[test]
    fn address_family_legacy_payload_defaults_to_auto() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("address_family");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.address_family, AddressFamily::Auto);
    }

    #[test]
    fn address_family_round_trips() {
        let mut conn = Connection::new("v6-host", "host.example");
        conn.address_family = AddressFamily::V6;
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.address_family, AddressFamily::V6);
    }

    #[test]
    fn protocol_telnet_round_trips() {
        let mut conn = Connection::new("router", "192.168.0.1");
        conn.protocol = ConnectionProtocol::Telnet;
        conn.port = 23;
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.protocol, ConnectionProtocol::Telnet);
    }

    /// A peer that never heard of Telnet-over-TLS sends no `telnet`
    /// key. It must land as plain Telnet, never as "TLS on with
    /// verification skipped": an absent field is not consent to stop
    /// checking certificates.
    #[test]
    fn telnet_options_legacy_payload_defaults_to_plain() {
        let mut conn = Connection::new("legacy", "10.0.0.1");
        conn.protocol = ConnectionProtocol::Telnet;
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("telnet");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.telnet, None);
    }

    #[test]
    fn telnet_options_round_trip() {
        let mut conn = Connection::new("switch", "10.0.0.1");
        conn.protocol = ConnectionProtocol::Telnet;
        conn.port = 992;
        conn.telnet = Some(super::super::telnet::TelnetOptions { tls: true, tls_insecure: true });
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        let opts = de.telnet.expect("options survive the round trip");
        assert!(opts.tls);
        assert!(opts.tls_insecure);
    }

    #[test]
    fn local_config_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("local");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.local, None);
    }

    #[test]
    fn local_config_round_trips() {
        let mut conn = Connection::new("Claude", "");
        conn.protocol = ConnectionProtocol::Local;
        conn.initial_command = Some("claude".to_string());
        conn.local = Some(super::super::local::LocalConfig {
            terminal_id: Some(Uuid::nil()),
            terminal_label: Some("PowerShell".to_string()),
            cwd: Some("~/work".to_string()),
        });
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        let local = de.local.expect("local config survives the round trip");
        assert_eq!(local.terminal_label.as_deref(), Some("PowerShell"));
        assert_eq!(local.effective_cwd(), Some("~/work"));
        assert_eq!(de.initial_command.as_deref(), Some("claude"));
    }

    /// The port field is shown by `uses_network_port` and seeded by
    /// `default_port`, which are NOT the same question: Raw dials a
    /// port and has no conventional one (console servers map each line
    /// to its own), so it must show the field and suggest nothing.
    #[test]
    fn raw_takes_a_port_but_suggests_none() {
        assert!(ConnectionProtocol::Raw.uses_network_port());
        assert_eq!(ConnectionProtocol::Raw.default_port(), None);
        assert!(!ConnectionProtocol::Serial.uses_network_port());
        assert!(!ConnectionProtocol::Local.uses_network_port());
        assert_eq!(ConnectionProtocol::Ssh.default_port(), Some(22));
        assert_eq!(ConnectionProtocol::Telnet.default_port(), Some(23));
    }

    /// Every terminal protocol round-trips through its quick-connect
    /// scheme, and `telnets` resolves to Telnet (the caller turns the
    /// TLS option on). `Local` names no endpoint, so it has no scheme.
    #[test]
    fn schemes_round_trip() {
        for p in [
            ConnectionProtocol::Ssh,
            ConnectionProtocol::Telnet,
            ConnectionProtocol::Raw,
            ConnectionProtocol::Serial,
            ConnectionProtocol::RemoteDesktop,
        ] {
            let scheme = p.scheme().expect("every dialable protocol names a scheme");
            assert_eq!(ConnectionProtocol::from_scheme(scheme), Some(p), "{scheme}");
            assert_eq!(
                ConnectionProtocol::from_scheme(&scheme.to_uppercase()),
                Some(p),
                "{scheme} uppercased"
            );
        }
        assert_eq!(ConnectionProtocol::Local.scheme(), None);
        assert_eq!(
            ConnectionProtocol::from_scheme("telnets"),
            Some(ConnectionProtocol::Telnet)
        );
        assert_eq!(ConnectionProtocol::from_scheme("http"), None);
    }

    #[test]
    fn keepalive_interval_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        conn.keepalive_interval = Some(45);
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.keepalive_interval, Some(45));

        // Explicit zero must round-trip distinctly from None, they have
        // different semantics (per-host disable vs. inherit global).
        conn.keepalive_interval = Some(0);
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.keepalive_interval, Some(0));
    }

    /// The legacy-cipher override fields are newest of all; a peer / export
    /// without them must default every category to `None` (= Auto).
    #[test]
    fn algorithm_overrides_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        let obj = value.as_object_mut().unwrap();
        for f in ["ciphers", "kex", "macs", "host_key_algorithms"] {
            obj.remove(f);
        }
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.ciphers, None);
        assert_eq!(de.kex, None);
        assert_eq!(de.macs, None);
        assert_eq!(de.host_key_algorithms, None);
    }

    #[test]
    fn algorithm_overrides_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        conn.ciphers = Some(vec!["aes256-cbc".into(), "3des-cbc".into()]);
        conn.kex = Some(vec!["diffie-hellman-group14-sha1".into()]);
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.ciphers.as_deref(), Some(&["aes256-cbc".to_string(), "3des-cbc".to_string()][..]));
        assert_eq!(de.kex.as_deref(), Some(&["diffie-hellman-group14-sha1".to_string()][..]));
        assert_eq!(de.macs, None);
    }

    #[test]
    fn auto_title_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("auto_title");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.auto_title, None);
    }

    #[test]
    fn terminal_type_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("terminal_type");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.terminal_type, None);
    }

    #[test]
    fn quirks_legacy_payload_defaults_to_none() {
        // A payload from before C5 has neither field; both must default
        // to None (all-xterm behaviour), keeping old vaults / peers valid.
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("quirks");
        obj.remove("rekey_limit_mb");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.quirks, None);
        assert_eq!(de.rekey_limit_mb, None);
    }

    #[test]
    fn quirks_payload_without_option_as_meta_defaults_to_none() {
        // Quirks JSON written before the option_as_meta field existed must
        // deserialize with the composing default, not fail.
        use super::super::terminal_quirks::{OptionAsMeta, TerminalQuirks};
        let mut conn = Connection::new("h", "1.2.3.4");
        conn.quirks = Some(TerminalQuirks::default());
        let mut value = serde_json::to_value(&conn).unwrap();
        value["quirks"]
            .as_object_mut()
            .unwrap()
            .remove("option_as_meta");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.quirks.unwrap().option_as_meta, OptionAsMeta::None);
    }

    #[test]
    fn quirks_round_trip() {
        use super::super::terminal_quirks::{
            BackspaceMode, FunctionKeyMode, HomeEndMode, OptionAsMeta, Osc52Override,
            TerminalQuirks,
        };
        let mut conn = Connection::new("h", "1.2.3.4");
        conn.quirks = Some(TerminalQuirks {
            backspace: BackspaceMode::CtrlH,
            home_end: HomeEndMode::Rxvt,
            function_keys: FunctionKeyMode::Vt400,
            disable_mouse_reporting: true,
            disable_title_change: true,
            osc52: Some(Osc52Override::Off),
            option_as_meta: OptionAsMeta::OnlyLeft,
        });
        conn.rekey_limit_mb = Some(256);
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.quirks, conn.quirks);
        assert_eq!(de.rekey_limit_mb, Some(256));
    }

    #[test]
    fn auto_title_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        // None (inherit), Some(true) (force on), Some(false) (force off) must
        // each round-trip distinctly, they have different semantics.
        for v in [None, Some(true), Some(false)] {
            conn.auto_title = v;
            let json = serde_json::to_string(&conn).unwrap();
            let de: Connection = serde_json::from_str(&json).unwrap();
            assert_eq!(de.auto_title, v);
        }
    }

    #[test]
    fn privacy_mode_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("privacy_mode");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.privacy_mode, None);
    }

    #[test]
    fn sftp_initial_path_legacy_payload_defaults_to_none() {
        // Old peers and old vault rows carry no such field; a missing one
        // must read as "land in the login directory", never an error.
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("sftp_initial_path");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.sftp_initial_path, None);
    }

    #[test]
    fn sftp_initial_path_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        for v in [None, Some(String::new()), Some("/srv/www".to_string())] {
            conn.sftp_initial_path = v.clone();
            let json = serde_json::to_string(&conn).unwrap();
            let de: Connection = serde_json::from_str(&json).unwrap();
            assert_eq!(de.sftp_initial_path, v);
        }
    }

    #[test]
    fn zmodem_drops_legacy_payload_defaults_to_false() {
        // Old peers and old vault rows carry no such field; a missing one
        // must read as "standard drop routing", never an error.
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("zmodem_drops");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert!(!de.zmodem_drops);
    }

    #[test]
    fn zmodem_drops_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        for v in [false, true] {
            conn.zmodem_drops = v;
            let json = serde_json::to_string(&conn).unwrap();
            let de: Connection = serde_json::from_str(&json).unwrap();
            assert_eq!(de.zmodem_drops, v);
        }
    }

    #[test]
    fn sidebar_auto_open_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        value.as_object_mut().unwrap().remove("sidebar_auto_open");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.sidebar_auto_open, None);
    }

    #[test]
    fn sidebar_auto_open_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        // None (inherit), Some(true) (force open), Some(false) (never
        // open) each round-trip distinctly.
        for v in [None, Some(true), Some(false)] {
            conn.sidebar_auto_open = v;
            let json = serde_json::to_string(&conn).unwrap();
            let de: Connection = serde_json::from_str(&json).unwrap();
            assert_eq!(de.sidebar_auto_open, v);
        }
    }

    #[test]
    fn privacy_mode_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        // None (inherit), Some(true) (force on), Some(false) (force off) must
        // each round-trip distinctly, they have different semantics.
        for v in [None, Some(true), Some(false)] {
            conn.privacy_mode = v;
            let json = serde_json::to_string(&conn).unwrap();
            let de: Connection = serde_json::from_str(&json).unwrap();
            assert_eq!(de.privacy_mode, v);
        }
    }

    #[test]
    fn auth_method_variants() {
        assert_eq!(serde_json::to_string(&AuthMethod::Auto).unwrap(), "\"Auto\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Password).unwrap(), "\"Password\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Key).unwrap(), "\"Key\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Agent).unwrap(), "\"Agent\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Interactive).unwrap(), "\"Interactive\"");
        assert_eq!(serde_json::to_string(&AuthMethod::PasswordPrompt).unwrap(), "\"PasswordPrompt\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Certificate).unwrap(), "\"Certificate\"");
        // And back: a synced payload from a newer peer must land on the
        // same variant, not silently degrade.
        assert_eq!(
            serde_json::from_str::<AuthMethod>("\"Certificate\"").unwrap(),
            AuthMethod::Certificate
        );
    }
}
