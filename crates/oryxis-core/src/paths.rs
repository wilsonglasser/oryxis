//! Central resolution of the app's data directory (`~/.oryxis` by
//! default).
//!
//! Four sources, the first that answers wins:
//!
//! 1. `ORYXIS_DATA_DIR` names the data tree ITSELF. Not a user setting:
//!    the app writes it into every MCP client entry when its own tree is
//!    not the default one, so `oryxis-mcp` (spawned by the client from
//!    `<tree>/bin`) reads the vaults the app shows.
//! 2. `ORYXIS_HOME` overrides the home directory the `.oryxis` tree
//!    lives under. It exists for the harness sandbox on Windows, where
//!    `dirs::home_dir()` is `SHGetKnownFolderPath` (a WinAPI call that
//!    ignores `$HOME` / `%USERPROFILE%`).
//! 3. A portable entry beside the program (portable mode, issue #271): the
//!    whole tree lives there and travels with it. "Beside the program" is
//!    the folder of the executable, of the `.AppImage` file, or of the
//!    `Oryxis.app` bundle ([`anchor_for`]), never the home folder itself
//!    (`~/data` is the user's own). The entry is `data` on Windows, where
//!    the zip's own folder holds it, and `oryxis-data` on Linux and macOS,
//!    where the program usually sits in a SHARED folder (`~/Downloads`,
//!    `~/Applications`, `/Applications`) that may hold anyone's `data`
//!    ([`PortableNaming`]). ANY entry by that name
//!    counts, a dangling junction or a file included: the user asked for
//!    that folder, and a tree that cannot be used there is reported by
//!    whatever fails to open in it, never swapped in silence for the
//!    profile's. The app's own start may CREATE it ([`settle_portable`]:
//!    the Windows portable zip, when the profile holds no tree yet and
//!    no package manager owns the folder).
//!    Decided ONCE per process, so a folder created while the app runs
//!    does not move the tree under it.
//! 4. `.oryxis` under the OS home.
//!
//! The environment wins over the folder: the harness sandbox has to hold
//! whatever sits beside the binary under test. Every source governs only
//! the app's own data tree: user files outside it (`~/.ssh/config`,
//! `~/.aws`, `~/.Xauthority`, the OS download folder) keep resolving
//! against the real OS home, so a portable vault never relocates the
//! user's own configuration.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The variable naming the data tree itself (source 1 above).
pub const DATA_DIR_ENV: &str = "ORYXIS_DATA_DIR";

/// The variable the app's WSL entry hands `oryxis-mcp.exe`: the distro the
/// client runs in, so a Linux path the client names can be read from
/// Windows (`\\wsl.localhost\<distro>\...`).
pub const WSL_DISTRO_ENV: &str = "ORYXIS_WSL_DISTRO";

/// The variable naming the home the `.oryxis` tree lives under
/// (source 2 above).
pub const HOME_ENV: &str = "ORYXIS_HOME";

/// The entry beside the executable that turns portable mode on, on
/// Windows (the one name the app creates).
pub const PORTABLE_DIR_NAME: &str = "data";

/// The entry that turns portable mode on beside a Linux binary, an
/// `.AppImage` or `Oryxis.app`. Named after the product, the way a portable
/// VS Code beside its bundle reads `code-portable-data`: the folder holding
/// the program is often shared, and a `data` there may be anyone's.
pub const PORTABLE_DIR_NAME_NAMED: &str = "oryxis-data";

/// The file every data tree holds once Oryxis has run in it
/// (`oryxis_vault::device`'s store), read here as the proof that a `data`
/// folder beside a Linux or macOS program is an Oryxis tree.
const DEVICE_FILE_NAME: &str = "device.json";

/// Which entry beside the program is the portable tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortableNaming {
    /// Windows: `data`, the folder the zip's copy creates in its own
    /// folder.
    Generic,
    /// Linux and macOS: `oryxis-data`; a `data` folder only when it
    /// already holds an Oryxis tree (one 0.23.0 picked up by the generic
    /// name).
    Named,
}

impl PortableNaming {
    fn host() -> Self {
        if cfg!(windows) { Self::Generic } else { Self::Named }
    }

    /// The entry a start that makes the folder creates.
    fn created_name(self) -> &'static str {
        match self {
            Self::Generic => PORTABLE_DIR_NAME,
            Self::Named => PORTABLE_DIR_NAME_NAMED,
        }
    }
}

/// Where the data tree came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDirSource {
    /// `ORYXIS_DATA_DIR`.
    DataDirEnv,
    /// `ORYXIS_HOME`.
    HomeEnv,
    /// The `data` folder beside the executable.
    Portable,
    /// `.oryxis` under the OS home: what every other process of this
    /// user finds with nothing set.
    OsHome,
}

impl DataDirSource {
    /// Short name for the diagnostic report.
    pub fn label(self) -> &'static str {
        match self {
            Self::DataDirEnv => DATA_DIR_ENV,
            Self::HomeEnv => HOME_ENV,
            Self::Portable => "portable",
            Self::OsHome => "profile",
        }
    }
}

/// The data tree and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    pub path: PathBuf,
    pub source: DataDirSource,
}

/// The data tree this process uses, `None` when nothing resolves (no
/// override, no portable folder, no OS home).
pub fn data_dir() -> Option<DataDir> {
    resolve_data_dir(
        std::env::var_os(DATA_DIR_ENV),
        std::env::var_os(HOME_ENV),
        portable_dir(),
        dirs::home_dir(),
    )
}

/// The data directory itself (vault, plugin cache, fonts, logs, tray
/// runtime, agent socket). Not created here; each consumer creates what
/// it needs on demand.
pub fn oryxis_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.path)
}

/// The tree a process spawned from somewhere else (an MCP client) has to
/// be TOLD about: `Some` unless the tree is the OS home's `.oryxis`,
/// which every process of this user finds on its own.
pub fn data_dir_for_children() -> Option<PathBuf> {
    data_dir().filter(|d| d.source != DataDirSource::OsHome).map(|d| d.path)
}

/// Whether the tree is a portable one: the `data` folder beside the
/// program.
pub fn is_portable() -> bool {
    data_dir().is_some_and(|d| d.source == DataDirSource::Portable)
}

/// The portable decision of this process: [`settle_portable`]'s, or the
/// passive probe's for a process that never settles (`oryxis-mcp`, the
/// cloud plugins, tests).
static PORTABLE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The portable folder of this process.
fn portable_dir() -> Option<PathBuf> {
    PORTABLE.get_or_init(|| match program_anchor() {
        Some(Anchor::Dir(dir)) => existing_data_entry(&dir),
        Some(Anchor::Translocated) | None => None,
    })
    .clone()
}

/// What [`settle_portable`] found, for the start's log.
#[derive(Debug)]
pub enum PortableStart {
    /// An override variable is set; the folder is not looked at.
    Overridden,
    /// The `data` folder was there.
    Found(PathBuf),
    /// The `data` folder was made by this start.
    Created(PathBuf),
    /// No `data` folder, and this copy does not make one (an installed
    /// build, a platform where portable mode is the user's to turn on).
    Off,
    /// No `data` folder, and the profile already holds a tree: a copy
    /// that ran before portable mode existed keeps it.
    ProfileKept(PathBuf),
    /// The `data` folder could not be made (a read-only folder, a copy
    /// run from inside the zip); the profile's tree is used. Nothing was
    /// there to lose.
    CreateFailed { path: PathBuf, error: String },
    /// macOS runs this copy from a randomized read-only location (App
    /// Translocation, a quarantined app not yet moved by Finder): the
    /// folder beside the bundle cannot be seen from here.
    Translocated,
    /// No `data` folder, and a package manager owns the folder (Scoop
    /// unpacks the same zip into one folder PER VERSION and removes the
    /// old ones): a tree made there would be left behind by the next
    /// update and deleted by the cleanup after it. The profile's tree is
    /// used, as an installed build's is.
    PackageManaged(PathBuf),
}

/// The app's start settles portable mode before anything touches the
/// tree. With `create_by_default` (the Windows portable zip: no
/// installer beside it) a copy with no `data` folder MAKES one, unless
/// the profile already holds a tree, which a zip copy older than this
/// mode wrote there and keeps using, or a package manager owns the
/// folder (Scoop: [`PortableStart::PackageManaged`]). Every other process only looks
/// ([`portable_dir`]). A second call answers what the first decided.
pub fn settle_portable(create_by_default: bool) -> PortableStart {
    let set = |v: Option<OsString>| v.is_some_and(|v| !v.is_empty());
    if set(std::env::var_os(DATA_DIR_ENV)) || set(std::env::var_os(HOME_ENV)) {
        let _ = PORTABLE.set(None);
        return PortableStart::Overridden;
    }
    let anchor = program_anchor();
    let (decision, outcome) = decide_portable(
        anchor,
        create_by_default,
        dirs::home_dir().map(|h| h.join(".oryxis")),
        PortableNaming::host(),
    );
    if PORTABLE.set(decision).is_err() {
        // Something read the tree before the start settled it: the
        // passive probe answered, and it stays (the tree never moves
        // inside a process).
        return match portable_dir() {
            Some(dir) => PortableStart::Found(dir),
            None => PortableStart::Off,
        };
    }
    outcome
}

/// [`settle_portable`] past the environment check.
fn decide_portable(
    anchor: Option<Anchor>,
    create_by_default: bool,
    profile_tree: Option<PathBuf>,
    naming: PortableNaming,
) -> (Option<PathBuf>, PortableStart) {
    let dir = match anchor {
        Some(Anchor::Dir(dir)) => dir,
        Some(Anchor::Translocated) => return (None, PortableStart::Translocated),
        None => return (None, PortableStart::Off),
    };
    if let Some(found) = portable_entry(&dir, naming) {
        return (Some(found.clone()), PortableStart::Found(found));
    }
    if !create_by_default {
        return (None, PortableStart::Off);
    }
    if is_package_managed(&dir) {
        return (None, PortableStart::PackageManaged(dir));
    }
    if let Some(profile) = profile_tree.filter(|p| p.symlink_metadata().is_ok()) {
        return (None, PortableStart::ProfileKept(profile));
    }
    let data = dir.join(naming.created_name());
    match std::fs::create_dir(&data) {
        Ok(()) => (Some(data.clone()), PortableStart::Created(data)),
        Err(e) => (None, PortableStart::CreateFailed { path: data, error: e.to_string() }),
    }
}

/// The portable entry in `dir`, when there is one.
fn existing_data_entry(dir: &Path) -> Option<PathBuf> {
    portable_entry(dir, PortableNaming::host())
}

/// [`existing_data_entry`] for a given naming. The named entry is taken by
/// `symlink_metadata` rather than `is_dir`: a junction to a volume that is
/// not mounted (a VeraCrypt drive) is still the user's choice of tree, and
/// following it would fall back to the profile in silence. A `data` folder
/// under [`PortableNaming::Named`] has to show it is an Oryxis tree (the
/// AppImage catalog's test downloads the image beside its own `data`).
fn portable_entry(dir: &Path, naming: PortableNaming) -> Option<PathBuf> {
    let any_entry = |name: &str| {
        let entry = dir.join(name);
        entry.symlink_metadata().is_ok().then_some(entry)
    };
    match naming {
        PortableNaming::Generic => any_entry(PORTABLE_DIR_NAME),
        PortableNaming::Named => any_entry(PORTABLE_DIR_NAME_NAMED).or_else(|| {
            let data = dir.join(PORTABLE_DIR_NAME);
            data.join(DEVICE_FILE_NAME).is_file().then_some(data)
        }),
    }
}

/// Whether a package manager owns `dir` and replaces it on update.
/// Scoop installs every version into `apps\<app>\<version>` (reached
/// through the `current` junction) and writes its `manifest.json` and
/// `install.json` there; a user's own unpacked copy has neither. Both
/// are asked for, so a stray `manifest.json` beside a copy is not enough.
fn is_package_managed(dir: &Path) -> bool {
    dir.join("install.json").is_file() && dir.join("manifest.json").is_file()
}

/// Where "beside the program" is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Anchor {
    /// The folder the `data` entry goes in.
    Dir(PathBuf),
    /// macOS App Translocation: no folder to look in.
    Translocated,
}

/// The anchor of this process, from its executable and the AppImage
/// runtime's variables.
fn program_anchor() -> Option<Anchor> {
    let exe = std::env::current_exe().ok()?;
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let appimage = var("APPIMAGE").zip(var("APPDIR"));
    let anchor = anchor_for(&exe, appimage.as_ref().map(|(image, dir)| (image.as_path(), dir.as_path())));
    outside_home(anchor, dirs::home_dir().as_deref())
}

/// A program kept straight in the home folder (an `.AppImage` in `~`,
/// `Oryxis.app` in `~`) has no portable folder: `~/data` is the user's
/// own, and taking it would move the whole tree there in silence. The
/// comparison follows links, so `/home/me` and a symlinked home agree.
fn outside_home(anchor: Option<Anchor>, home: Option<&Path>) -> Option<Anchor> {
    let same = |a: &Path, b: &Path| match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    };
    match (anchor, home) {
        (Some(Anchor::Dir(dir)), Some(home)) if same(&dir, home) => None,
        (anchor, _) => anchor,
    }
}

/// Pure core of [`program_anchor`]:
///
/// - an AppImage runs from a read-only mount of itself; the folder is the
///   one holding the `.AppImage` FILE (`APPIMAGE`). Honoured only when the
///   executable is inside the mount the runtime names (`APPDIR`): a
///   program started from a terminal some other AppImage opened inherits
///   that one's variables.
/// - a macOS bundle is `Oryxis.app/Contents/MacOS/<exe>`; the folder is
///   the one holding `Oryxis.app`, never inside the bundle (writing
///   there breaks its signature). A quarantined bundle macOS runs from
///   `.../AppTranslocation/...` has no such folder to look in.
/// - anything else: the executable's own folder.
fn anchor_for(exe: &Path, appimage: Option<(&Path, &Path)>) -> Option<Anchor> {
    if let Some((image, mount)) = appimage
        && exe.starts_with(mount)
    {
        return image.parent().map(|d| Anchor::Dir(d.to_path_buf()));
    }
    if exe.components().any(|c| c.as_os_str() == "AppTranslocation") {
        return Some(Anchor::Translocated);
    }
    let dir = exe.parent()?;
    if dir.file_name().is_some_and(|n| n == "MacOS")
        && let Some(contents) = dir.parent()
        && contents.file_name().is_some_and(|n| n == "Contents")
        && let Some(bundle) = contents.parent()
        && bundle.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
    {
        return bundle.parent().map(|d| Anchor::Dir(d.to_path_buf()));
    }
    Some(Anchor::Dir(dir.to_path_buf()))
}

/// `target` as a path relative to `base`, `..` steps included, when both
/// sit on the same volume: the same drive (or share) on Windows, the same
/// mount on Unix. A portable tree stores what it points at on its own
/// volume this way, so the key keeps working when it comes up as another
/// drive letter (or under another user's `/media/<user>`). `None` across
/// volumes (a VeraCrypt drive keeps its letter on purpose) or when either
/// path cannot be made absolute.
pub fn relative_on_same_volume(base: &Path, target: &Path) -> Option<PathBuf> {
    let base = std::path::absolute(base).ok()?;
    let target = std::path::absolute(target).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        // The nearest existing ancestor of each: a folder about to be
        // made is on the volume its parent is on.
        let dev = |p: &Path| p.ancestors().find_map(|a| std::fs::metadata(a).ok()).map(|m| m.dev());
        if dev(&base)? != dev(&target)? {
            return None;
        }
    }
    relative_lexically(&base, &target)
}

/// `relative` joined onto `base` with its `..` steps folded away, the
/// inverse of [`relative_on_same_volume`] (which works on the paths as
/// written, never through links): `E:\Oryxis\data` + `..\..\Vaults\
/// work.db` is `E:\Vaults\work.db`, which is also what a user should
/// see. A `..` past the root stays at the root.
pub fn join_lexically(base: &Path, relative: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in base.join(relative).components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(out.components().next_back(), None | Some(Component::RootDir | Component::Prefix(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The lexical half of [`relative_on_same_volume`]: both paths absolute,
/// `None` when their prefixes (drive, share) differ. Windows compares
/// names without case.
fn relative_lexically(base: &Path, target: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let same = |a: &Component, b: &Component| {
        if cfg!(windows) {
            a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
        } else {
            a == b
        }
    };
    let base: Vec<Component> = base.components().filter(|c| *c != Component::CurDir).collect();
    let target: Vec<Component> = target.components().filter(|c| *c != Component::CurDir).collect();
    let rooted = |c: &[Component]| c.iter().take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir)).count();
    let (base_root, target_root) = (rooted(&base), rooted(&target));
    if base_root == 0
        || base_root != target_root
        || !base[..base_root].iter().zip(&target[..target_root]).all(|(a, b)| same(a, b))
    {
        return None;
    }
    if base.iter().chain(&target).any(|c| *c == Component::ParentDir) {
        // `absolute` keeps `..` as written; resolving it lexically could
        // cross a link, so such a path is not made relative.
        return None;
    }
    let common = base.iter().zip(&target).take_while(|(a, b)| same(a, b)).count();
    let mut out = PathBuf::new();
    for _ in common..base.len() {
        out.push("..");
    }
    for c in &target[common..] {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        // The base itself: an empty string reads as "not set" wherever a
        // path is stored.
        out.push(".");
    }
    Some(out)
}

/// Pure core of [`data_dir`], split out so the resolution is
/// unit-testable without mutating the process environment (`set_var` is
/// unsafe under Rust 2024, and test binaries run their tests on
/// parallel threads).
fn resolve_data_dir(
    data_env: Option<OsString>,
    home_env: Option<OsString>,
    portable: Option<PathBuf>,
    os_home: Option<PathBuf>,
) -> Option<DataDir> {
    // An empty export must fall through, matching `dirs_sys`' own `HOME`
    // handling; otherwise the data dir would land in the process working
    // directory.
    let set = |v: Option<OsString>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(path) = set(data_env) {
        return Some(DataDir { path, source: DataDirSource::DataDirEnv });
    }
    if let Some(home) = set(home_env) {
        return Some(DataDir { path: home.join(".oryxis"), source: DataDirSource::HomeEnv });
    }
    if let Some(path) = portable {
        return Some(DataDir { path, source: DataDirSource::Portable });
    }
    os_home.map(|home| DataDir { path: home.join(".oryxis"), source: DataDirSource::OsHome })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(path: &str, source: DataDirSource) -> Option<DataDir> {
        Some(DataDir { path: PathBuf::from(path), source })
    }

    /// `ORYXIS_HOME` overrides the data directory's home. The harness
    /// sandbox depends on this on Windows, where `dirs::home_dir()` is
    /// a WinAPI call that ignores `$HOME` / `%USERPROFILE%`: without
    /// the override a harness run would read and write the REAL
    /// profile's `.oryxis`. Exercised through the pure resolver; the
    /// end-to-end pin lives in `oryxis-vault/tests/oryxis_home.rs`, a
    /// binary with exactly one test and therefore no `getenv` race.
    #[test]
    fn oryxis_home_overrides_home_dir() {
        let sandbox = || Some(OsString::from("/sandbox"));
        let home = || Some(PathBuf::from("/real-home"));
        // The override wins over the OS home.
        assert_eq!(
            resolve_data_dir(None, sandbox(), None, home()),
            tree("/sandbox/.oryxis", DataDirSource::HomeEnv)
        );
        // Unset: the OS home.
        assert_eq!(resolve_data_dir(None, None, None, home()), tree("/real-home/.oryxis", DataDirSource::OsHome));
        // An accidental `export ORYXIS_HOME=` falls through to the real
        // home instead of landing the data dir in the working directory.
        assert_eq!(
            resolve_data_dir(None, Some(OsString::new()), None, home()),
            tree("/real-home/.oryxis", DataDirSource::OsHome)
        );
        // Nothing resolves at all: consumers surface their own errors.
        assert_eq!(resolve_data_dir(Some(OsString::new()), Some(OsString::new()), None, None), None);
    }

    /// The portable folder beats the OS home and loses to both variables:
    /// the harness sandbox (`ORYXIS_HOME`) must hold over a `data` folder
    /// beside the binary under test, and `oryxis-mcp`, which lives in
    /// `<tree>/bin`, finds the app's tree only through `ORYXIS_DATA_DIR`.
    #[test]
    fn portable_folder_sits_between_the_variables_and_the_home() {
        let portable = || Some(PathBuf::from("E:/Oryxis/data"));
        let home = || Some(PathBuf::from("C:/Users/me"));
        assert_eq!(
            resolve_data_dir(None, None, portable(), home()),
            tree("E:/Oryxis/data", DataDirSource::Portable)
        );
        // No OS home at all still finds the portable tree.
        assert_eq!(resolve_data_dir(None, None, portable(), None), tree("E:/Oryxis/data", DataDirSource::Portable));
        assert_eq!(
            resolve_data_dir(None, Some(OsString::from("/sandbox")), portable(), home()),
            tree("/sandbox/.oryxis", DataDirSource::HomeEnv)
        );
        // The tree variable names the folder itself, no `.oryxis` added,
        // and wins over everything.
        assert_eq!(
            resolve_data_dir(
                Some(OsString::from("E:/Oryxis/data")),
                Some(OsString::from("/sandbox")),
                Some(PathBuf::from("F:/elsewhere/data")),
                home()
            ),
            tree("E:/Oryxis/data", DataDirSource::DataDirEnv)
        );
        // An empty `ORYXIS_DATA_DIR` is unset, like an empty `ORYXIS_HOME`.
        assert_eq!(
            resolve_data_dir(Some(OsString::new()), None, portable(), home()),
            tree("E:/Oryxis/data", DataDirSource::Portable)
        );
    }

    /// Portable mode is the named entry beside the program, in any
    /// form: a folder, a link whose target is gone (a VeraCrypt volume
    /// not mounted yet), even a file. Each of those is the user asking
    /// for that tree; what fails to open there reports itself, and the
    /// profile's tree is never used in its place.
    #[test]
    fn portable_folder_is_any_data_entry_beside_the_program() {
        for (naming, name) in
            [(PortableNaming::Generic, PORTABLE_DIR_NAME), (PortableNaming::Named, PORTABLE_DIR_NAME_NAMED)]
        {
            let root = tempdir("entry");
            assert_eq!(portable_entry(&root, naming), None);

            std::fs::create_dir(root.join(name)).unwrap();
            assert_eq!(portable_entry(&root, naming), Some(root.join(name)));
            std::fs::remove_dir(root.join(name)).unwrap();

            std::fs::write(root.join(name), b"").unwrap();
            assert_eq!(portable_entry(&root, naming), Some(root.join(name)));
            std::fs::remove_file(root.join(name)).unwrap();

            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(root.join("unmounted"), root.join(name)).unwrap();
                assert_eq!(portable_entry(&root, naming), Some(root.join(name)));
            }
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// Beside a Linux binary, an `.AppImage` or `Oryxis.app` the folder is
    /// often shared: a `data` folder there is someone else's (the AppImage
    /// catalog's test downloads the image next to its own `data/`) unless
    /// it already holds an Oryxis tree, and `oryxis-data` wins over it.
    #[test]
    fn a_shared_folders_data_is_taken_only_when_it_is_an_oryxis_tree() {
        let root = tempdir("named");
        std::fs::create_dir(root.join(PORTABLE_DIR_NAME)).unwrap();
        std::fs::write(root.join(PORTABLE_DIR_NAME).join("Oryxis"), b"https://example.org").unwrap();
        assert_eq!(portable_entry(&root, PortableNaming::Named), None);
        // Windows takes it: that folder is the zip's own.
        assert_eq!(portable_entry(&root, PortableNaming::Generic), Some(root.join(PORTABLE_DIR_NAME)));

        std::fs::write(root.join(PORTABLE_DIR_NAME).join(DEVICE_FILE_NAME), b"{}").unwrap();
        assert_eq!(portable_entry(&root, PortableNaming::Named), Some(root.join(PORTABLE_DIR_NAME)));

        std::fs::create_dir(root.join(PORTABLE_DIR_NAME_NAMED)).unwrap();
        assert_eq!(portable_entry(&root, PortableNaming::Named), Some(root.join(PORTABLE_DIR_NAME_NAMED)));
        let _ = std::fs::remove_dir_all(&root);

        // A start that makes the folder makes the name it would read back.
        let fresh = tempdir("named-make");
        let (made, _) = decide_portable(Some(Anchor::Dir(fresh.clone())), true, None, PortableNaming::Named);
        assert_eq!(made, Some(fresh.join(PORTABLE_DIR_NAME_NAMED)));
        let _ = std::fs::remove_dir_all(&fresh);
    }

    /// A program kept straight in the home folder never reads `~/data`
    /// as its tree (it is the user's own folder); one in a folder below
    /// the home, or elsewhere, keeps its anchor. A link to the home is
    /// the home.
    #[test]
    fn a_program_in_the_home_folder_has_no_portable_folder() {
        let root = tempdir("home");
        let home = root.join("me");
        let apps = home.join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::create_dir(home.join(PORTABLE_DIR_NAME)).unwrap();
        let at = |d: &Path| Some(Anchor::Dir(d.to_path_buf()));

        assert_eq!(outside_home(at(&home), Some(&home)), None);
        assert_eq!(outside_home(at(&apps), Some(&home)), at(&apps));
        assert_eq!(outside_home(at(&home), None), at(&home));
        assert_eq!(outside_home(Some(Anchor::Translocated), Some(&home)), Some(Anchor::Translocated));
        assert_eq!(outside_home(None, Some(&home)), None);
        #[cfg(unix)]
        {
            let link = root.join("home-link");
            std::os::unix::fs::symlink(&home, &link).unwrap();
            assert_eq!(outside_home(at(&link), Some(&home)), None);
            assert_eq!(outside_home(at(&home), Some(&link)), None);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    fn tempdir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("oryxis-portable-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// "Beside the program" is the executable's folder, the folder of the
    /// `.AppImage` file (never the read-only mount it runs from), or the
    /// folder of the `.app` bundle (never inside it); a translocated
    /// bundle has none.
    #[test]
    fn the_anchor_follows_how_the_program_is_packaged() {
        let dir = |p: &str| Some(Anchor::Dir(PathBuf::from(p)));
        assert_eq!(anchor_for(Path::new("/media/usb/Oryxis/oryxis"), None), dir("/media/usb/Oryxis"));
        // AppImage: the image file's folder.
        let mounted = Path::new("/tmp/.mount_OryxisAbc/usr/bin/oryxis");
        let image = (Path::new("/media/usb/Oryxis.AppImage"), Path::new("/tmp/.mount_OryxisAbc"));
        assert_eq!(anchor_for(mounted, Some(image)), dir("/media/usb"));
        // Variables inherited from ANOTHER AppImage: ignored.
        assert_eq!(anchor_for(Path::new("/opt/x/oryxis"), Some(image)), dir("/opt/x"));
        // macOS bundle: the folder holding it.
        assert_eq!(
            anchor_for(Path::new("/Volumes/USB/Oryxis.app/Contents/MacOS/oryxis"), None),
            dir("/Volumes/USB")
        );
        // A bare binary in a folder that merely looks like one.
        assert_eq!(anchor_for(Path::new("/x/MacOS/oryxis"), None), dir("/x/MacOS"));
        assert_eq!(
            anchor_for(
                Path::new("/private/var/folders/ab/T/AppTranslocation/1234/d/Oryxis.app/Contents/MacOS/oryxis"),
                None
            ),
            Some(Anchor::Translocated)
        );
    }

    /// The start's decision: a `data` entry is always taken; without one,
    /// only a copy that makes it by default (the Windows zip) makes it,
    /// and only when the profile holds no tree a pre-portable copy wrote.
    #[test]
    fn a_start_takes_makes_or_leaves_the_portable_folder() {
        let root = tempdir("decide");
        let profile = root.join("home").join(".oryxis");
        let app = root.join("app");
        std::fs::create_dir_all(&app).unwrap();
        let anchor = || Some(Anchor::Dir(app.clone()));

        // Opt-in platforms: nothing there, nothing made.
        assert!(matches!(decide_portable(anchor(), false, Some(profile.clone()), PortableNaming::Generic), (None, PortableStart::Off)));
        assert!(!app.join(PORTABLE_DIR_NAME).exists());
        // The profile holds a tree: kept, nothing made.
        std::fs::create_dir_all(&profile).unwrap();
        assert!(matches!(decide_portable(anchor(), true, Some(profile.clone()), PortableNaming::Generic), (None, PortableStart::ProfileKept(_))));
        assert!(!app.join(PORTABLE_DIR_NAME).exists());
        std::fs::remove_dir_all(&profile).unwrap();
        // A fresh copy: made.
        let (made, outcome) = decide_portable(anchor(), true, Some(profile.clone()), PortableNaming::Generic);
        assert_eq!(made, Some(app.join(PORTABLE_DIR_NAME)));
        assert!(matches!(outcome, PortableStart::Created(_)));
        // From then on it is found, whatever the profile holds and on
        // every platform.
        std::fs::create_dir_all(&profile).unwrap();
        assert!(matches!(decide_portable(anchor(), false, Some(profile.clone()), PortableNaming::Generic), (Some(_), PortableStart::Found(_))));
        // A folder that cannot hold it: the profile, said so.
        let missing = || Some(Anchor::Dir(root.join("gone")));
        assert!(matches!(decide_portable(missing(), true, None, PortableNaming::Generic), (None, PortableStart::CreateFailed { .. })));
        assert!(matches!(decide_portable(Some(Anchor::Translocated), true, None, PortableNaming::Generic), (None, PortableStart::Translocated)));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A Scoop install unpacks the portable zip into a folder per version
    /// and deletes the old ones on cleanup: a fresh user's first start
    /// must not put the tree there, or the next update strands it and
    /// the cleanup after it deletes the vault. A `data` folder the user
    /// made there is still theirs to use.
    #[test]
    fn a_package_managed_copy_never_makes_the_portable_folder() {
        let root = tempdir("scoop");
        let profile = root.join("home").join(".oryxis");
        let app = root.join("scoop").join("apps").join("oryxis").join("0.23.0");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("manifest.json"), b"{}").unwrap();
        std::fs::write(app.join("install.json"), b"{}").unwrap();
        let anchor = || Some(Anchor::Dir(app.clone()));

        let (made, outcome) = decide_portable(anchor(), true, Some(profile.clone()), PortableNaming::Generic);
        assert_eq!(made, None);
        assert!(matches!(outcome, PortableStart::PackageManaged(_)), "{outcome:?}");
        assert!(!app.join(PORTABLE_DIR_NAME).exists());

        // One of the two files alone is not Scoop's folder.
        std::fs::remove_file(app.join("install.json")).unwrap();
        assert!(matches!(decide_portable(anchor(), true, Some(profile.clone()), PortableNaming::Generic), (Some(_), PortableStart::Created(_))));
        std::fs::write(app.join("install.json"), b"{}").unwrap();
        // The folder the user made (or the one just made) is found.
        assert!(matches!(decide_portable(anchor(), true, Some(profile.clone()), PortableNaming::Generic), (Some(_), PortableStart::Found(_))));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A path on the tree's own volume is stored relative to the tree, so
    /// a new drive letter (or another `/media/<user>`) does not break it;
    /// another volume keeps its absolute path.
    #[test]
    fn a_path_on_the_trees_volume_is_relative_to_it() {
        if cfg!(windows) {
            let rel = |a: &str, b: &str| relative_lexically(Path::new(a), Path::new(b));
            assert_eq!(rel("E:\\Oryxis\\data", "E:\\Oryxis\\data\\vaults\\x.db"), Some(PathBuf::from("vaults\\x.db")));
            assert_eq!(rel("E:\\Oryxis\\data", "e:\\Vaults\\work.db"), Some(PathBuf::from("..\\..\\Vaults\\work.db")));
            assert_eq!(rel("E:\\Oryxis\\data", "V:\\work.db"), None);
            assert_eq!(rel("E:\\Oryxis\\data", "E:\\Oryxis\\data"), Some(PathBuf::from(".")));
        } else {
            let rel = |a: &str, b: &str| relative_lexically(Path::new(a), Path::new(b));
            assert_eq!(rel("/media/u/USB/Oryxis/data", "/media/u/USB/Oryxis/data/vaults/x.db"), Some(PathBuf::from("vaults/x.db")));
            assert_eq!(rel("/media/u/USB/Oryxis/data", "/media/u/USB/Vaults/work.db"), Some(PathBuf::from("../../Vaults/work.db")));
            assert_eq!(rel("/media/u/USB/Oryxis/data", "/media/u/USB/Oryxis/data"), Some(PathBuf::from(".")));
            assert_eq!(rel("/a/b", "/a/../c"), None, "a `..` as written is not resolved");
            assert_eq!(rel("a/b", "/a/c"), None, "a relative base");
            for (base, target) in [("/media/u/USB/Oryxis/data", "/media/u/USB/Vaults/work.db"), ("/a/b", "/a/b/c/d")] {
                let relative = rel(base, target).unwrap();
                assert_eq!(join_lexically(Path::new(base), &relative), PathBuf::from(target), "the round trip");
            }
            assert_eq!(join_lexically(Path::new("/a"), Path::new("../../x")), PathBuf::from("/x"));
            // Two mounts: the device numbers differ, never relative.
            let root = tempdir("volume");
            assert_eq!(relative_on_same_volume(&root, &root.join("x.db")), Some(PathBuf::from("x.db")));
            if Path::new("/proc/self").exists() {
                assert_eq!(relative_on_same_volume(&root, Path::new("/proc/self/status")), None);
            }
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// The diagnostic report names the source without the path (which
    /// carries the user's name): the variable, or the mode.
    #[test]
    fn source_labels_name_every_source() {
        assert_eq!(DataDirSource::DataDirEnv.label(), "ORYXIS_DATA_DIR");
        assert_eq!(DataDirSource::HomeEnv.label(), "ORYXIS_HOME");
        assert_eq!(DataDirSource::Portable.label(), "portable");
        assert_eq!(DataDirSource::OsHome.label(), "profile");
    }
}
