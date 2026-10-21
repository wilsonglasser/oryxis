//! Boot-time GPU health probe for the "auto" renderer mode (Windows).
//!
//! wgpu's default backend set is `Backends::PRIMARY` (Vulkan | DX12 |
//! Metal); the GL backend is never tried automatically. On Haswell-era
//! Intel iGPUs (4th-gen Core, HD 4200-5200) that leaves no healthy
//! path on Windows: Intel never shipped a Windows Vulkan driver for
//! that generation and disabled DX12 entirely in driver 15.40.44+ (the
//! CVE-2019-14615 mitigation), while the older drivers that still
//! expose DX12 composite undecorated windows offset by a phantom title
//! bar (content shifted down, black band on top, clicks landing above
//! the visuals). When DX12 is disabled, wgpu still enumerates WARP
//! ("Microsoft Basic Render Driver"), a software rasterizer.
//!
//! So, before iced builds its compositor, look at what Vulkan/DX12
//! actually offer. If every adapter is a software rasterizer, or every
//! hardware adapter is a Haswell iGPU, redirect "auto" to iced's
//! tiny-skia software renderer, which presents via GDI and is always
//! correct. GL (WGL) is deliberately NOT the fallback: a field report
//! (HD 4400, Windows 10) showed the same phantom-titlebar offset on
//! the hardware GL path, so on this driver generation every
//! GPU-accelerated present is suspect. Software rendering is usable
//! for our workload since the fork's tiny-skia clip-mask memoization
//! (iced#3368) removed the dense-text lag.
//!
//! The probe only inspects adapters (no device, no surface), so it is
//! cheap and cannot trip the swapchain bugs it is routing around. The
//! caller gates it to Windows at runtime; Linux/macOS keep wgpu's
//! defaults (the GNOME + Mesa corruption case stays a manual setting).

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll, Wake, Waker};

/// Set to `true` at boot when [`auto_backend_override`] decided this GPU
/// stack is unhealthy and redirected the renderer. Read later (post
/// unlock) to auto-enable performance mode once on this hardware. A
/// process global rather than threaded state so both the locked and
/// auto-unlocked boot paths see the same signal. Defaults to `false`
/// (never set on healthy stacks or non-probed platforms).
static PROBE_REDIRECTED: OnceLock<bool> = OnceLock::new();

/// Records that the boot probe redirected the renderer away from the
/// default GPU path. Idempotent; only the first call sticks.
pub fn mark_redirected() {
    let _ = PROBE_REDIRECTED.set(true);
}

/// Whether the boot probe redirected the renderer to a safer backend on
/// this hardware. Drives the one-time performance-mode auto-enable.
pub fn probe_redirected() -> bool {
    *PROBE_REDIRECTED.get_or_init(|| false)
}

/// A backend redirect decided by the probe: the environment variable
/// iced/wgpu read while building the compositor, plus a human-readable
/// reason for the log.
pub struct BackendOverride {
    pub env_key: &'static str,
    pub env_value: &'static str,
    pub reason: String,
}

/// Inspect the Vulkan/DX12 adapters and decide whether "auto" should
/// be redirected to a safer backend. `None` means the default path is
/// healthy (or the user already forced a backend via environment).
pub fn auto_backend_override() -> Option<BackendOverride> {
    // An explicit env override (user, script, or our own panic-hook
    // relaunch) always wins over the probe.
    if std::env::var_os("WGPU_BACKEND").is_some() || std::env::var_os("ICED_BACKEND").is_some() {
        return None;
    }

    let primary = wgpu::Backends::VULKAN | wgpu::Backends::DX12;
    let defect = primary_defect(&enumerate(primary))?;

    // The primary path is broken. GL is not a safe alternative on
    // this hardware class (see module docs), so go straight to the
    // software renderer, which always presents correctly.
    Some(BackendOverride {
        env_key: "ICED_BACKEND",
        env_value: "tiny-skia",
        reason: format!("{defect}; redirecting the auto renderer to software (tiny-skia)"),
    })
}

/// Inspect the GL adapters when the user explicitly selected the
/// OpenGL renderer, and decide whether the "GPU-accelerated" promise
/// of that option can actually be kept. wgpu's GL backend on top of a
/// software rasterizer (llvmpipe on a WSLg session whose vGPU died,
/// SwiftShader, WARP-GL) is its least mature combination and
/// misrenders quads, background fills go missing and content leaks
/// outside clips, while being no faster than the real software
/// renderer. Redirect to tiny-skia, which is correct AND faster on a
/// CPU. An empty enumeration is inconclusive (headless EGL quirks),
/// so only positive evidence of a software-only GL stack redirects.
pub fn opengl_backend_override() -> Option<BackendOverride> {
    // An explicit env override always wins, same rule as the auto probe.
    if std::env::var_os("WGPU_BACKEND").is_some() || std::env::var_os("ICED_BACKEND").is_some() {
        return None;
    }

    let defect = gl_defect(&enumerate(wgpu::Backends::GL))?;
    Some(BackendOverride {
        env_key: "ICED_BACKEND",
        env_value: "tiny-skia",
        reason: format!(
            "{defect}: the OpenGL selection would run software-rasterized GL, which \
             misrenders; using the software renderer (tiny-skia) instead"
        ),
    })
}

/// Why the explicit-OpenGL selection cannot deliver hardware GL, or
/// `None` when it can (or when we cannot tell).
fn gl_defect(adapters: &[AdapterSummary]) -> Option<String> {
    if adapters.is_empty() {
        // Inconclusive: surfaceless EGL enumeration can fail where a
        // windowed context would work. Let the real boot try.
        return None;
    }
    if adapters.iter().all(|a| !a.is_hardware) {
        return Some(format!(
            "no hardware OpenGL adapter, only software rasterizers ({})",
            names(adapters.iter())
        ));
    }
    None
}

/// The slice of [`wgpu::AdapterInfo`] the decision logic needs, split
/// out so the logic stays testable without a GPU (and stable against
/// `AdapterInfo` gaining fields).
struct AdapterSummary {
    name: String,
    vendor: u32,
    device: u32,
    /// `false` for software rasterizers (WARP, llvmpipe, SwiftShader),
    /// which wgpu reports as [`wgpu::DeviceType::Cpu`].
    is_hardware: bool,
}

/// Enumerate the adapters of `backends` into summaries.
fn enumerate(backends: wgpu::Backends) -> Vec<AdapterSummary> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        // Adapter enumeration needs no display; the compositor iced
        // builds later creates its own instance with the real window.
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    block_on(instance.enumerate_adapters(backends))
        .into_iter()
        .map(|adapter| {
            let info = adapter.get_info();
            AdapterSummary {
                name: info.name,
                vendor: info.vendor,
                device: info.device,
                is_hardware: info.device_type != wgpu::DeviceType::Cpu,
            }
        })
        .collect()
}

/// Why the Vulkan/DX12 adapter set cannot be trusted, or `None` when
/// it looks healthy.
fn primary_defect(adapters: &[AdapterSummary]) -> Option<String> {
    let hardware: Vec<&AdapterSummary> = adapters.iter().filter(|a| a.is_hardware).collect();
    if hardware.is_empty() {
        return Some(if adapters.is_empty() {
            "no Vulkan/DX12 adapter at all".to_string()
        } else {
            format!(
                "no hardware Vulkan/DX12 adapter, only software rasterizers ({})",
                names(adapters.iter())
            )
        });
    }
    if hardware.iter().all(|a| is_haswell_igpu(a.vendor, a.device)) {
        return Some(format!(
            "every hardware Vulkan/DX12 adapter is a Haswell-era Intel iGPU ({}), \
             whose EOL drivers present undecorated windows offset",
            names(hardware.iter().copied())
        ));
    }
    None
}

fn names<'a>(adapters: impl Iterator<Item = &'a AdapterSummary>) -> String {
    adapters
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

const INTEL_VENDOR_ID: u32 = 0x8086;

/// Whether a PCI (vendor, device) pair is a Haswell-generation Intel
/// iGPU. Haswell graphics device IDs (per Mesa's PCI ID tables) live
/// in four families, 0x04xx (desktop), 0x0Axx (ULT), 0x0Cxx (SDV) and
/// 0x0Dxx (Crystal Well / Iris Pro), where the second nibble encodes
/// the GT tier (0/1/2) and the last one the form factor
/// (2/6/A/B/E = desktop/mobile/server/reserved/embedded).
fn is_haswell_igpu(vendor: u32, device: u32) -> bool {
    if vendor != INTEL_VENDOR_ID {
        return false;
    }
    let family = device & 0xFF00;
    let gt_tier = (device & 0x00F0) >> 4;
    let form = device & 0x000F;
    matches!(family, 0x0400 | 0x0A00 | 0x0C00 | 0x0D00)
        && gt_tier <= 2
        && matches!(form, 0x2 | 0x6 | 0xA | 0xB | 0xE)
}

/// Minimal single-future executor. `Instance::enumerate_adapters` is
/// async only for wasm parity and resolves immediately on native
/// backends, but poll it properly (park + wake) instead of assuming
/// readiness, so a future wgpu that really suspends still works.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(name: &str, vendor: u32, device: u32, is_hardware: bool) -> AdapterSummary {
        AdapterSummary {
            name: name.to_string(),
            vendor,
            device,
            is_hardware,
        }
    }

    #[test]
    fn recognizes_haswell_igpus() {
        // HD 4600 desktop / mobile, HD 4400, HD 4200, Iris Pro 5200.
        for device in [0x0412, 0x0416, 0x0A16, 0x0A1E, 0x0D26] {
            assert!(
                is_haswell_igpu(INTEL_VENDOR_ID, device),
                "0x{device:04X} is Haswell"
            );
        }
    }

    #[test]
    fn rejects_non_haswell_devices() {
        // Ivy Bridge HD 4000, Broadwell HD 5500, Skylake HD 530,
        // Kaby Lake UHD 620, Arc A770, and an NVIDIA vendor id.
        for (vendor, device) in [
            (INTEL_VENDOR_ID, 0x0166),
            (INTEL_VENDOR_ID, 0x1616),
            (INTEL_VENDOR_ID, 0x1912),
            (INTEL_VENDOR_ID, 0x5917),
            (INTEL_VENDOR_ID, 0x56A0),
            (0x10DE, 0x0416),
        ] {
            assert!(
                !is_haswell_igpu(vendor, device),
                "{vendor:04X}:{device:04X} is not Haswell"
            );
        }
    }

    #[test]
    fn warp_only_is_a_defect() {
        let adapters = [adapter("Microsoft Basic Render Driver", 0x1414, 0x008C, false)];
        let defect = primary_defect(&adapters).expect("WARP-only must be flagged");
        assert!(defect.contains("software rasterizers"));
        assert!(defect.contains("Microsoft Basic Render Driver"));
    }

    #[test]
    fn no_adapters_is_a_defect() {
        assert!(primary_defect(&[]).is_some());
    }

    #[test]
    fn haswell_only_is_a_defect() {
        // Old driver still exposing hardware DX12 on an HD 4600, with
        // WARP alongside (the usual DX12 enumeration on that setup).
        let adapters = [
            adapter("Intel(R) HD Graphics 4600", INTEL_VENDOR_ID, 0x0416, true),
            adapter("Microsoft Basic Render Driver", 0x1414, 0x008C, false),
        ];
        let defect = primary_defect(&adapters).expect("Haswell-only must be flagged");
        assert!(defect.contains("Haswell"));
    }

    #[test]
    fn gl_software_only_is_a_defect() {
        // A WSLg session whose vGPU died: GL enumerates llvmpipe only.
        // The explicit-OpenGL selection must fall back to tiny-skia.
        let adapters = [adapter("llvmpipe (LLVM 20.1.2, 256 bits)", 0xFFFF, 0, false)];
        let defect = gl_defect(&adapters).expect("software-only GL must be flagged");
        assert!(defect.contains("llvmpipe"));
    }

    #[test]
    fn gl_hardware_is_not_flagged() {
        let adapters = [
            adapter("D3D12 (NVIDIA GeForce RTX 3060)", 0x10DE, 0x2503, true),
            adapter("llvmpipe (LLVM 20.1.2, 256 bits)", 0xFFFF, 0, false),
        ];
        assert!(gl_defect(&adapters).is_none());
    }

    #[test]
    fn gl_empty_enumeration_is_inconclusive() {
        // Surfaceless EGL can fail to enumerate where a windowed
        // context would work; never redirect without positive evidence.
        assert!(gl_defect(&[]).is_none());
    }

    #[test]
    fn healthy_hardware_is_not_flagged() {
        // A discrete GPU next to the Haswell iGPU means the default
        // backend pick has a healthy adapter to land on.
        let adapters = [
            adapter("Intel(R) HD Graphics 4600", INTEL_VENDOR_ID, 0x0416, true),
            adapter("NVIDIA GeForce GTX 1650", 0x10DE, 0x1F82, true),
        ];
        assert!(primary_defect(&adapters).is_none());

        // And so does any modern single GPU.
        let modern = [adapter("Intel(R) UHD Graphics 620", INTEL_VENDOR_ID, 0x5917, true)];
        assert!(primary_defect(&modern).is_none());
    }
}
