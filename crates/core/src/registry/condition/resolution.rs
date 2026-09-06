use serde::Deserialize;
use serde_json::{Value, json};

use crate::facts::FileFacts;
use crate::registry::ConditionField;
use crate::registry::condition::parse;

/// `resolution` — matches the file's video resolution by pixel count.
///
/// Flow JSON: `{ "resolution": { "min": [3840, 2160], "max": [1920, 1080] } }`.
/// A bound `[w, h]` means "at least / at most `w*h` pixels" (inclusive),
/// so aspect ratio is irrelevant — `≥ 4K` catches 3840×2160 and anything
/// larger. Files without video never match a non-"any" constraint.
///
/// The schema advertises `common` resolutions as type-ahead suggestions;
/// the UI accepts free text ("1080p", "4k", "1920×1080") and stores the
/// pixel bound, so no fixed option list is required.
pub struct Resolution;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    /// Minimum resolution, `[width, height]`.
    min: Option<[u32; 2]>,
    /// Maximum resolution, `[width, height]`.
    max: Option<[u32; 2]>,
}

impl Constraint {
    fn is_any(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }
}

impl ConditionField for Resolution {
    fn key(&self) -> &'static str {
        "resolution"
    }

    fn description(&self) -> &'static str {
        "Video resolution, compared by pixel count (min/max bounds)"
    }

    fn match_facts(&self, constraint: &Value, facts: &FileFacts) -> crate::error::Result<bool> {
        let c: Constraint = parse(self.key(), constraint)?;
        if c.is_any() {
            return Ok(true);
        }
        let Some(v) = facts.video() else {
            return Ok(false);
        };
        let px = v.pixels();
        if let Some([w, h]) = c.min {
            if px < w as u64 * h as u64 {
                return Ok(false);
            }
        }
        if let Some([w, h]) = c.max {
            if px > w as u64 * h as u64 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn ui_schema(&self) -> Value {
        json!({
            "kind": "resolution_range",
            "label": "Resolution",
            "common": {
                "720p": [1280, 720],
                "1080p": [1920, 1080],
                "1440p": [2560, 1440],
                "4k": [3840, 2160]
            },
            "hint": "Type a resolution (e.g. 1080p, 4k) or width × height. Bounds are inclusive pixel counts; leave empty for any."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::VideoFacts;

    fn facts(w: u32, h: u32) -> FileFacts {
        FileFacts {
            container: "mkv".into(),
            video: Some(VideoFacts {
                codec: "hevc".into(),
                width: w,
                height: h,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn min_bound() {
        let c = Resolution;
        let v = json!({ "min": [3840, 2160] });
        assert!(c.match_facts(&v, &facts(3840, 2160)).unwrap());
        assert!(!c.match_facts(&v, &facts(1920, 1080)).unwrap());
    }

    #[test]
    fn max_bound() {
        let c = Resolution;
        let v = json!({ "max": [1920, 1080] });
        assert!(c.match_facts(&v, &facts(1920, 1080)).unwrap());
        assert!(!c.match_facts(&v, &facts(3840, 2160)).unwrap());
    }

    #[test]
    fn band() {
        let c = Resolution;
        let v = json!({ "min": [1920, 1080], "max": [3840, 2160] });
        assert!(c.match_facts(&v, &facts(2560, 1440)).unwrap());
        assert!(!c.match_facts(&v, &facts(1280, 720)).unwrap());
        assert!(!c.match_facts(&v, &facts(4096, 2160)).unwrap());
    }

    #[test]
    fn pixel_count_not_dimensions() {
        let c = Resolution;
        // 2560×1080 has more pixels than 1920×1080 despite lower height.
        let v = json!({ "min": [1920, 1080] });
        assert!(c.match_facts(&v, &facts(2560, 1080)).unwrap());
    }

    #[test]
    fn no_video_never_matches() {
        let c = Resolution;
        let v = json!({ "min": [1920, 1080] });
        assert!(
            !c.match_facts(
                &v,
                &FileFacts {
                    container: "mp3".into(),
                    ..Default::default()
                }
            )
            .unwrap()
        );
    }
}
