//! Fusion rules, no Bevy: what each item gives, the urn's totals, and which
//! upgrades are ready. Data in `assets/data/fusion.ron`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Kind {
    pub name: String,
    pub title: String,
    pub color: (f32, f32, f32),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Upgrade {
    pub name: String,
    pub title: String,
    /// Fusion this upgrade spends, by kind.
    pub needs: BTreeMap<String, f32>,
    /// Upgrades that must come first.
    #[serde(default)]
    pub after: Vec<String>,
    /// The station it places beside the urn.
    pub station: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FusionData {
    pub kinds: Vec<Kind>,
    /// Item name -> fusion per item, by kind.
    pub items: BTreeMap<String, BTreeMap<String, f32>>,
    pub upgrades: Vec<Upgrade>,
}

impl FusionData {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let d: FusionData = ron::from_str(text).map_err(|e| format!("fusion.ron: {e}"))?;
        for (item, gives) in &d.items {
            for k in gives.keys() {
                if !d.kinds.iter().any(|x| &x.name == k) {
                    return Err(format!("fusion.ron: item '{item}' gives unknown fusion '{k}'"));
                }
            }
        }
        for u in &d.upgrades {
            for k in u.needs.keys() {
                if !d.kinds.iter().any(|x| &x.name == k) {
                    return Err(format!("fusion.ron: upgrade '{}' needs unknown fusion '{k}'", u.name));
                }
            }
            for a in &u.after {
                if !d.upgrades.iter().any(|x| &x.name == a) {
                    return Err(format!("fusion.ron: upgrade '{}' comes after unknown '{a}'", u.name));
                }
            }
        }
        Ok(d)
    }

    pub fn kind(&self, name: &str) -> Option<&Kind> {
        self.kinds.iter().find(|k| k.name == name)
    }

    /// What one of this item gives, if the urn takes it.
    pub fn of(&self, item: &str) -> Option<&BTreeMap<String, f32>> {
        self.items.get(item)
    }
}

/// What the urn holds and what it has built. Saved with the world.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Urn {
    pub fusion: BTreeMap<String, f32>,
    /// Upgrades done, in the order they happened.
    pub done: Vec<String>,
}

impl Urn {
    pub fn get(&self, kind: &str) -> f32 {
        self.fusion.get(kind).copied().unwrap_or(0.0)
    }

    /// Put `count` of an item in. Returns false (and changes nothing) if the
    /// urn doesn't take it.
    pub fn deposit(&mut self, data: &FusionData, item: &str, count: u32) -> bool {
        let Some(gives) = data.of(item) else { return false };
        for (k, v) in gives {
            *self.fusion.entry(k.clone()).or_insert(0.0) += v * count as f32;
        }
        true
    }

    pub fn has(&self, upgrade: &str) -> bool {
        self.done.iter().any(|d| d == upgrade)
    }

    /// The next upgrade to work toward: the first not yet done whose
    /// prerequisites are.
    pub fn next<'a>(&self, data: &'a FusionData) -> Option<&'a Upgrade> {
        data.upgrades.iter().find(|u| !self.has(&u.name) && u.after.iter().all(|a| self.has(a)))
    }

    /// How far along the next upgrade is, 0..1 (the least-filled kind counts).
    pub fn progress(&self, data: &FusionData) -> f32 {
        self.next(data).map_or(1.0, |u| u.needs.iter().map(|(k, n)| (self.get(k) / n).min(1.0)).fold(1.0, f32::min))
    }

    /// Do every upgrade that's ready, spending its fusion. Returns them in order.
    pub fn upgrade(&mut self, data: &FusionData) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(u) = self.next(data) {
            if !u.needs.iter().all(|(k, n)| self.get(k) + 1e-4 >= *n) {
                break;
            }
            for (k, n) in &u.needs {
                *self.fusion.entry(k.clone()).or_insert(0.0) -= n;
            }
            self.done.push(u.name.clone());
            out.push(u.name.clone());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> FusionData {
        FusionData::from_ron(include_str!("../../../../assets/data/fusion.ron")).unwrap()
    }

    #[test]
    fn ten_sticks_place_the_tool_bench() {
        let d = data();
        let mut urn = Urn::default();
        for i in 0..9 {
            assert!(urn.deposit(&d, "stick", 1));
            assert!(urn.upgrade(&d).is_empty(), "too early after {} sticks", i + 1);
        }
        assert!((urn.progress(&d) - 0.9).abs() < 1e-4);
        urn.deposit(&d, "stick", 1);
        assert_eq!(urn.upgrade(&d), vec!["tool_bench".to_string()]);
        assert_eq!(urn.get("earth"), 0.0);
        assert_eq!(urn.next(&d).map(|u| u.name.as_str()), Some("carpentry_bench"));
    }

    #[test]
    fn about_ten_logs_place_the_carpentry_bench() {
        let d = data();
        let mut urn = Urn::default();
        urn.deposit(&d, "stick", 10);
        urn.upgrade(&d);
        urn.deposit(&d, "log", 10);
        assert_eq!(urn.upgrade(&d), vec!["carpentry_bench".to_string()]);
        assert!(urn.next(&d).is_none());
    }

    #[test]
    fn the_urn_refuses_what_has_no_fusion() {
        let d = data();
        let mut urn = Urn::default();
        assert!(!urn.deposit(&d, "torch", 1));
        assert_eq!(urn, Urn::default());
    }

    #[test]
    fn every_item_in_fusion_ron_exists() {
        let d = data();
        let items = crate::items::tests::load().1;
        for name in d.items.keys() {
            assert!(items.id(name).is_some(), "fusion.ron lists unknown item '{name}'");
        }
    }
}
