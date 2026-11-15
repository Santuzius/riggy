//! A Unity `.anim` file (text YAML) as its float curves and clip settings: what a humanoid clip holds (muscle curves and the body's `RootQ`).

use std::collections::BTreeMap;

/// A key of a Hermite curve.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    time: f64,
    value: f64,
    in_slope: f64,
    out_slope: f64,
}

/// A clip's float curves by attribute, and the part of it that plays.
#[derive(Debug, Default)]
pub struct Anim {
    curves: BTreeMap<String, Vec<Key>>,
    pub start: f64,
    pub stop: f64,
    /// Whether the clip plays mirrored (`m_Mirror`).
    pub mirror: bool,
}

impl Anim {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut anim = Anim::default();
        let mut section = "";
        let mut keys: Vec<Key> = Vec::new();
        let mut key: Option<Key> = None;
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("  m_").and_then(|rest| rest.split(':').next()) {
                section = match name {
                    "FloatCurves" => "float",
                    "AnimationClipSettings" => "settings",
                    _ => "",
                };
            }
            let trimmed = line.trim_start().trim_start_matches("- ");
            let Some((field, value)) = trimmed.split_once(':') else { continue };
            let value = value.trim();
            match section {
                "float" => match field {
                    "time" => {
                        keys.extend(key.take());
                        key = Some(Key { time: number(value)?, value: 0.0, in_slope: 0.0, out_slope: 0.0 });
                    }
                    "value" => key.as_mut().ok_or("a value before its time")?.value = number(value)?,
                    "inSlope" => key.as_mut().ok_or("a slope before its time")?.in_slope = number(value)?,
                    "outSlope" => key.as_mut().ok_or("a slope before its time")?.out_slope = number(value)?,
                    "attribute" => {
                        keys.extend(key.take());
                        anim.curves.insert(value.to_owned(), std::mem::take(&mut keys));
                    }
                    _ => {}
                },
                "settings" => match field {
                    "m_StartTime" => anim.start = number(value)?,
                    "m_StopTime" => anim.stop = number(value)?,
                    "m_Mirror" => anim.mirror = value == "1",
                    _ => {}
                },
                _ => {}
            }
        }
        if anim.curves.is_empty() {
            return Err("no float curves (not a humanoid clip, or a binary .anim)".into());
        }
        if anim.stop <= anim.start {
            anim.stop = anim.curves.values().filter_map(|k| k.last()).map(|k| k.time).fold(0.0, f64::max);
        }
        Ok(anim)
    }

    pub fn attributes(&self) -> impl Iterator<Item = &str> {
        self.curves.keys().map(String::as_str)
    }

    /// The curve `attribute` at `time`, `None` without such a curve.
    pub fn value(&self, attribute: &str, time: f64) -> Option<f64> {
        self.curves.get(attribute).map(|keys| evaluate(keys, time))
    }
}

/// A Hermite curve at `t`, held at its ends; an infinite slope (a stepped key) counts as flat.
fn evaluate(keys: &[Key], t: f64) -> f64 {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else { return 0.0 };
    if t <= first.time {
        return first.value;
    }
    if t >= last.time {
        return last.value;
    }
    let i = keys.partition_point(|k| k.time <= t) - 1;
    let (a, b) = (keys[i], keys[i + 1]);
    let dt = b.time - a.time;
    let s = (t - a.time) / dt;
    let slope = |x: f64| if x.abs() > 1e30 { 0.0 } else { x };
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * a.value + (s3 - 2.0 * s2 + s) * dt * slope(a.out_slope) + (-2.0 * s3 + 3.0 * s2) * b.value + (s3 - s2) * dt * slope(b.in_slope)
}

fn number(text: &str) -> Result<f64, String> {
    match text {
        "Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        _ => text.parse().map_err(|_| format!("not a number: {text}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIP: &str = "AnimationClip:
  m_Name: Idle
  m_FloatCurves:
  - serializedVersion: 2
    curve:
      serializedVersion: 2
      m_Curve:
      - serializedVersion: 3
        time: 0
        value: 0
        inSlope: 0
        outSlope: 0
      - serializedVersion: 3
        time: 1
        value: 1
        inSlope: 0
        outSlope: 0
    attribute: Spine Front-Back
    path:
  m_PPtrCurves: []
  m_AnimationClipSettings:
    m_StartTime: 0
    m_StopTime: 1
    m_Mirror: 1
";

    #[test]
    fn reads_curves_and_settings() {
        let anim = Anim::parse(CLIP).unwrap();
        assert_eq!(anim.attributes().collect::<Vec<_>>(), ["Spine Front-Back"]);
        assert!(anim.mirror);
        assert_eq!(anim.stop, 1.0);
        assert_eq!(anim.value("Spine Front-Back", 0.5), Some(0.5));
        assert_eq!(anim.value("Spine Front-Back", 2.0), Some(1.0));
    }
}
