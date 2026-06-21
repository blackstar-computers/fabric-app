//! Per-run time series from `GET /api/runs/series`.

use serde::{de::MapAccess, de::Visitor, Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Downsampled metric columns aligned to `epochs` (mirrors `web_app/src/types.ts` RunSeries).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RunSeries {
    pub pod: String,
    pub name: String,
    pub epochs: Vec<i64>,
    /// Metric column name → parallel values (`loss`, `top1`, `gnorm`, extras…).
    pub metrics: HashMap<String, Vec<f64>>,
}

impl RunSeries {
    pub const RESERVED: &'static [&'static str] = &["pod", "name", "epochs", "epoch", "n_series"];

    pub fn nums(&self, key: &str) -> &[f64] {
        self.metrics.get(key).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn latest(&self, key: &str) -> Option<f64> {
        series_latest(self.nums(key))
    }

    /// Value of `key` at the probe nearest `epoch` (port of `web_app` `valueAtEpoch`).
    /// Falls back to the latest finite value when there is no epoch axis to index.
    pub fn value_at_epoch(&self, key: &str, epoch: i64) -> Option<f64> {
        let vals = self.nums(key);
        if self.epochs.is_empty() || vals.is_empty() {
            return self.latest(key);
        }
        let i = nearest_epoch_index(&self.epochs, epoch)?;
        vals.get(i).copied().filter(|v| v.is_finite())
    }
}

pub fn series_latest(values: &[f64]) -> Option<f64> {
    values
        .iter()
        .rev()
        .find(|v| v.is_finite())
        .copied()
}

pub fn nearest_epoch_index(epochs: &[i64], target: i64) -> Option<usize> {
    if epochs.is_empty() {
        return None;
    }
    let mut best = 0usize;
    let mut best_d = i64::MAX;
    for (i, e) in epochs.iter().enumerate() {
        let d = (*e - target).abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    Some(best)
}

impl<'de> Deserialize<'de> for RunSeries {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(RunSeriesVisitor)
    }
}

struct RunSeriesVisitor;

impl<'de> Visitor<'de> for RunSeriesVisitor {
    type Value = RunSeries;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a run series object")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut pod = String::new();
        let mut name = String::new();
        let mut epochs = Vec::new();
        let mut metrics = HashMap::new();

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "pod" => pod = map.next_value()?,
                "name" => name = map.next_value()?,
                "epochs" => {
                    let val: serde_json::Value = map.next_value()?;
                    epochs = decode_i64_array(val);
                }
                "epoch" | "n_series" => {
                    let _ = map.next_value::<serde_json::Value>()?;
                }
                _ => {
                    let val: serde_json::Value = map.next_value()?;
                    if let Some(nums) = decode_num_array(val) {
                        if !nums.is_empty() {
                            metrics.insert(key, nums);
                        }
                    }
                }
            }
        }

        Ok(RunSeries {
            pod,
            name,
            epochs,
            metrics,
        })
    }
}

fn decode_i64_array(value: serde_json::Value) -> Vec<i64> {
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| v.as_i64().or_else(|| v.as_f64().map(|n| n as i64)))
        .collect()
}

fn decode_num_array(value: serde_json::Value) -> Option<Vec<f64>> {
    let arr = value.as_array()?;
    let nums: Vec<f64> = arr
        .iter()
        .filter_map(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
        .collect();
    if nums.is_empty() {
        None
    } else {
        Some(nums)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_series_fixture() {
        let json = r#"{
            "pod": "copytest:n1",
            "name": "membands4_n1",
            "epochs": [1000, 2000],
            "loss": [0.5, 0.3],
            "top1": [20.1, 22.0]
        }"#;
        let s: RunSeries = serde_json::from_str(json).expect("parse");
        assert_eq!(s.pod, "copytest:n1");
        assert_eq!(s.epochs, vec![1000, 2000]);
        assert_eq!(s.latest("loss"), Some(0.3));
    }

    #[test]
    fn value_at_epoch_picks_nearest_probe() {
        let json = r#"{
            "pod": "p", "name": "r",
            "epochs": [1000, 2000, 3000],
            "loss": [0.5, 0.3, 0.1]
        }"#;
        let s: RunSeries = serde_json::from_str(json).expect("parse");
        assert_eq!(s.value_at_epoch("loss", 1900), Some(0.3));
        assert_eq!(s.value_at_epoch("loss", 3000), Some(0.1));
        // missing key falls back to latest (None here)
        assert_eq!(s.value_at_epoch("ppl", 1000), None);
    }
}
