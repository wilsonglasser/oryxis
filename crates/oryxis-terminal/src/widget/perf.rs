/// Rolling per-frame samples for the perf overlay. We track the
/// **max** of each phase over a short window so transient spikes
/// (the kind that actually feel like lag) stay visible for a beat
/// instead of being averaged away.
pub(crate) struct PerfStats {
    /// Last few frames of each phase. Old entries are dropped after
    /// `WINDOW` so the max reflects recent activity, not the whole
    /// session.
    pub(crate) samples: std::collections::VecDeque<PerfSample>,
    /// Wall-clock of the previous draw, used so the overlay can
    /// avoid double-counting frames within a single redraw cycle.
    pub(crate) last_draw_at: Option<std::time::Instant>,
}

#[derive(Clone, Copy)]
pub(crate) struct PerfSample {
    pub(crate) frame_gap: std::time::Duration,
    pub(crate) lock: std::time::Duration,
    pub(crate) cells: std::time::Duration,
    pub(crate) highlights: std::time::Duration,
    pub(crate) total: std::time::Duration,
}

/// Frames retained for the rolling max / fps. ~2s of activity at
/// 60 fps; long enough to catch a typing burst, short enough that
/// the HUD recovers when things calm down.
pub(crate) const PERF_WINDOW: usize = 120;

impl PerfStats {
    pub(crate) fn fps(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let total: std::time::Duration =
            self.samples.iter().map(|s| s.frame_gap).sum();
        let avg = total / self.samples.len() as u32;
        if avg.as_secs_f32() == 0.0 {
            0.0
        } else {
            1.0 / avg.as_secs_f32()
        }
    }

    pub(crate) fn max_lock(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.lock)
            .max()
            .unwrap_or_default()
    }
    pub(crate) fn max_cells(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.cells)
            .max()
            .unwrap_or_default()
    }
    pub(crate) fn max_highlights(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.highlights)
            .max()
            .unwrap_or_default()
    }
    pub(crate) fn max_total(&self) -> std::time::Duration {
        self.samples
            .iter()
            .map(|s| s.total)
            .max()
            .unwrap_or_default()
    }
}

pub(crate) fn perf_stats() -> &'static std::sync::Mutex<PerfStats> {
    static STATS: std::sync::OnceLock<std::sync::Mutex<PerfStats>> =
        std::sync::OnceLock::new();
    STATS.get_or_init(|| {
        std::sync::Mutex::new(PerfStats {
            samples: std::collections::VecDeque::with_capacity(PERF_WINDOW),
            last_draw_at: None,
        })
    })
}

/// Reads the `ORYXIS_TERM_PERF` env var once and caches it. Set to `1`
/// (or any non-empty value) to render a small FPS/timing HUD in the
/// top-right of every terminal canvas.
pub(crate) fn perf_overlay_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("ORYXIS_TERM_PERF")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}
