use serde::{Deserialize, Serialize};

/// All public geometry is in logical desktop coordinates, matching the shared
/// UI's CSS coordinate contract. Native adapters alone convert physical units.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Bounds {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Bounds {
    pub(crate) fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && self.width >= 1.0
            && self.height >= 1.0
            && self.width <= 32768.0
            && self.height <= 32768.0
    }

    pub(crate) fn fit(self, work_area: Self) -> Self {
        let width = self.width.min(work_area.width).max(1.0);
        let height = self.height.min(work_area.height).max(1.0);
        Self {
            x: self
                .x
                .clamp(work_area.x, work_area.x + work_area.width - width),
            y: self
                .y
                .clamp(work_area.y, work_area.y + work_area.height - height),
            width,
            height,
        }
    }

    fn overlap(self, other: Self) -> f64 {
        let width = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
        let height = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
        width.max(0.0) * height.max(0.0)
    }

    /// Prefer the display containing the stored window; after its removal use
    /// the primary work area rather than leaving the only restore control offscreen.
    pub(crate) fn restore(self, work_areas: &[Self]) -> Option<Self> {
        let first = *work_areas.first()?;
        let area = work_areas.iter().copied().fold(first, |best, area| {
            if self.overlap(area) > self.overlap(best) {
                area
            } else {
                best
            }
        });
        Some(self.fit(area))
    }
}

#[cfg(test)]
mod tests {
    use super::Bounds;

    #[test]
    fn restores_to_remaining_display_after_monitor_removal() {
        let window = Bounds {
            x: 2200.0,
            y: 500.0,
            width: 640.0,
            height: 480.0,
        };
        let screen = Bounds {
            x: 0.0,
            y: 24.0,
            width: 1920.0,
            height: 1016.0,
        };
        assert_eq!(
            window.restore(&[screen]),
            Some(Bounds {
                x: 1280.0,
                y: 500.0,
                width: 640.0,
                height: 480.0,
            })
        );
    }

    #[test]
    fn preserves_negative_origin_and_shrinks_for_small_work_area() {
        let window = Bounds {
            x: -1800.0,
            y: 10.0,
            width: 900.0,
            height: 1200.0,
        };
        let left = Bounds {
            x: -1280.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        let primary = Bounds {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert_eq!(
            window.restore(&[primary, left]),
            Some(Bounds {
                x: -1280.0,
                y: 0.0,
                width: 900.0,
                height: 720.0,
            })
        );
    }
}
