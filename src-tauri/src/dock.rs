//! Where the pill lives: as the notch at the top centre of a monitor, or, after a Ctrl+drag,
//! docked flat against the left, right or top edge of any monitor. While it's being dragged it's
//! a round bubble; dropped, it sticks to the nearest of those edges.
//!
//! Kept in its own file rather than in settings: it changes by dragging, not in the Settings
//! window, so a Settings window that's open can't save a stale copy over it.

use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// The bottom edge is left out on purpose: that's where the taskbar is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    Top,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Docked {
    /// The monitor's name as the OS reports it; if it's unplugged, the notch's monitor is used.
    pub monitor: Option<String>,
    pub edge: Edge,
    /// Where along the edge, from 0 (left or top end of the work area) to 1 (right or bottom).
    pub along: f64,
}

pub struct DockStore {
    path: PathBuf,
    current: RwLock<Option<Docked>>,
}

impl DockStore {
    /// A missing or unreadable file (or one from an older version) means the notch.
    pub fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let path = app.path().app_config_dir()?.join("dock.json");
        let current = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Option<Docked>>(&text).ok())
            .flatten()
            .map(|d| Docked { along: if d.along.is_finite() { d.along.clamp(0.0, 1.0) } else { 0.5 }, ..d });
        Ok(Self { path, current: RwLock::new(current) })
    }

    /// `None` while it's the notch at the top centre.
    pub fn get(&self) -> Option<Docked> {
        self.current.read().unwrap().clone()
    }

    pub fn set(&self, docked: Option<Docked>) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&self.path, serde_json::to_string(&docked).unwrap_or_default()) {
            eprintln!("hoverpill: can't save {}: {e}", self.path.display());
        }
        *self.current.write().unwrap() = docked;
    }
}

/// Where a drop at `cursor` lands, all in physical px: the nearest of the work area's left, right
/// and top edges and how far along it, or `None` for the top centre, where it's the notch again.
/// `snap` is how far either side of the monitor's centre counts as the top centre.
pub fn drop_target(work: (i32, i32, i32, i32), monitor: (i32, i32, i32, i32), cursor: (f64, f64), snap: f64) -> Option<(Edge, f64)> {
    let (wx, wy, ww, wh) = (work.0 as f64, work.1 as f64, work.2 as f64, work.3 as f64);
    let (x, y) = cursor;
    let distances = [(Edge::Top, y - wy), (Edge::Left, x - wx), (Edge::Right, wx + ww - x)];
    let (edge, _) = distances.into_iter().fold((Edge::Top, f64::INFINITY), |best, d| if d.1 < best.1 { d } else { best });
    let centre = monitor.0 as f64 + monitor.2 as f64 / 2.0;
    if edge == Edge::Top && (x - centre).abs() <= snap {
        return None;
    }
    let along = match edge {
        Edge::Left | Edge::Right => (y - wy) / wh.max(1.0),
        Edge::Top => (x - wx) / ww.max(1.0),
    };
    Some((edge, along.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 1920×1080 monitor with a 48 px taskbar at the bottom.
    const MONITOR: (i32, i32, i32, i32) = (0, 0, 1920, 1080);
    const WORK: (i32, i32, i32, i32) = (0, 0, 1920, 1032);

    #[test]
    fn drops_on_the_nearest_edge() {
        assert_eq!(drop_target(WORK, MONITOR, (40.0, 500.0), 160.0), Some((Edge::Left, 500.0 / 1032.0)));
        assert_eq!(drop_target(WORK, MONITOR, (1900.0, 258.0), 160.0), Some((Edge::Right, 0.25)));
        assert_eq!(drop_target(WORK, MONITOR, (300.0, 20.0), 160.0), Some((Edge::Top, 300.0 / 1920.0)));
    }

    #[test]
    fn never_the_bottom() {
        // Near the bottom: whichever of the other edges is nearest.
        assert_eq!(drop_target(WORK, MONITOR, (480.0, 1000.0), 160.0), Some((Edge::Left, 1000.0 / 1032.0)));
        assert_eq!(drop_target(WORK, MONITOR, (1500.0, 1000.0), 160.0).map(|t| t.0), Some(Edge::Right));
        // On the taskbar: the nearer side, at the very end of the work area's span.
        assert_eq!(drop_target(WORK, MONITOR, (2000.0, 1060.0), 160.0), Some((Edge::Right, 1.0)));
    }

    #[test]
    fn the_top_centre_is_the_notch() {
        assert_eq!(drop_target(WORK, MONITOR, (960.0, 30.0), 160.0), None);
        assert_eq!(drop_target(WORK, MONITOR, (1100.0, 5.0), 160.0), None);
        // Near the top but well off centre: docked on the top edge there.
        assert_eq!(drop_target(WORK, MONITOR, (1300.0, 5.0), 160.0).map(|t| t.0), Some(Edge::Top));
    }

    #[test]
    fn a_second_monitor_uses_its_own_centre() {
        let monitor = (1920, 0, 2560, 1440);
        let work = (1920, 0, 2560, 1392);
        assert_eq!(drop_target(work, monitor, (3200.0, 10.0), 160.0), None);
        assert_eq!(drop_target(work, monitor, (1960.0, 700.0), 160.0).map(|t| t.0), Some(Edge::Left));
    }
}
