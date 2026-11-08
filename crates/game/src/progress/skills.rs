//! The skill tree (`assets/data/skills.ron`) and its rules. Plain Rust: a
//! server can check the same rules later.
//!
//! Major nodes link to each other with weighted edges; each major is ringed
//! by 8 minor nodes. The weight of an edge is the points it costs to take the
//! node at its end. A major can be taken once a linked major is (the root
//! always can). Taking a minor closes the two next to it in the ring.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const RING: usize = 8;
/// Distance of the ring from its major, in map units.
pub const RING_RADIUS: f32 = 95.0;

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Spell {
    Levitation,
    Light,
    Warmth,
}

impl Spell {

    pub fn title(self) -> &'static str {
        match self {
            Spell::Levitation => "Levitation",
            Spell::Light => "Light",
            Spell::Warmth => "Warmth",
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Gives {
    Carry(f32),
    Air(f32),
    Cold(f32),
    Speed(f32),
    Mining(f32),
    Damage(f32),
    Focus(f32),
    FocusRegen(f32),
    Spell(Spell),
    LevitateCost(f32),
}

impl Gives {
    pub fn describe(&self) -> String {
        match *self {
            Gives::Carry(kg) => format!("+{kg:.0} kg carried"),
            Gives::Air(a) => format!("+{:.0}% breath in thin air", a * 100.0),
            Gives::Cold(c) => format!("shrug off {c:.0}°C of cold"),
            Gives::Speed(s) => format!("+{:.0}% speed", s * 100.0),
            Gives::Mining(m) => format!("+{:.0}% mining speed", m * 100.0),
            Gives::Damage(d) => format!("+{:.0}% damage", d * 100.0),
            Gives::Focus(f) => format!("+{f:.0} focus"),
            Gives::FocusRegen(r) => format!("+{r:.1} focus per second"),
            Gives::Spell(s) => format!("spell: {}", s.title()),
            Gives::LevitateCost(c) => format!("levitating costs {:.0}% less", c * 100.0),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Minor {
    pub title: String,
    pub cost: u32,
    pub gives: Vec<Gives>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Major {
    pub name: String,
    pub title: String,
    pub pos: (f32, f32),
    pub gives: Vec<Gives>,
    pub ring: Vec<Minor>,
}

#[derive(Deserialize, Debug, Clone)]
struct TreeSpec {
    root: String,
    majors: Vec<Major>,
    links: Vec<(String, String, u32)>,
}

/// A node: a major, or minor `i` of major `m`'s ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Node {
    Major(usize),
    Minor(usize, usize),
}

#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct Tree {
    pub root: usize,
    pub majors: Vec<Major>,
    /// (a, b, weight), both ways.
    pub links: Vec<(usize, usize, u32)>,
}

impl Tree {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let spec: TreeSpec = ron::from_str(text).map_err(|e| format!("skills.ron: {e}"))?;
        let find = |n: &str| spec.majors.iter().position(|m| m.name == n).ok_or_else(|| format!("skills.ron: no major '{n}'"));
        for m in &spec.majors {
            if m.ring.len() != RING {
                return Err(format!("skills.ron: '{}' has {} ring nodes, not {RING}", m.name, m.ring.len()));
            }
        }
        let links = spec.links.iter().map(|(a, b, w)| Ok((find(a)?, find(b)?, *w))).collect::<Result<Vec<_>, String>>()?;
        Ok(Self { root: find(&spec.root)?, majors: spec.majors.clone(), links })
    }

    pub fn nodes(&self) -> impl Iterator<Item = Node> + '_ {
        (0..self.majors.len()).flat_map(|m| std::iter::once(Node::Major(m)).chain((0..RING).map(move |i| Node::Minor(m, i))))
    }

    pub fn title(&self, n: Node) -> &str {
        match n {
            Node::Major(m) => &self.majors[m].title,
            Node::Minor(m, i) => &self.majors[m].ring[i].title,
        }
    }

    pub fn gives(&self, n: Node) -> &[Gives] {
        match n {
            Node::Major(m) => &self.majors[m].gives,
            Node::Minor(m, i) => &self.majors[m].ring[i].gives,
        }
    }

    /// Where a node sits on the map.
    pub fn pos(&self, n: Node) -> (f32, f32) {
        match n {
            Node::Major(m) => self.majors[m].pos,
            Node::Minor(m, i) => {
                let (x, y) = self.majors[m].pos;
                let a = i as f32 / RING as f32 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                (x + a.cos() * RING_RADIUS, y + a.sin() * RING_RADIUS)
            }
        }
    }

    /// The weight of the edge into a node from what has been taken, if it can be reached.
    pub fn cost(&self, taken: &BTreeSet<Node>, n: Node) -> Option<u32> {
        match n {
            Node::Major(m) if m == self.root => Some(1),
            Node::Major(m) => self
                .links
                .iter()
                .filter_map(|&(a, b, w)| {
                    let other = if a == m { b } else if b == m { a } else { return None };
                    taken.contains(&Node::Major(other)).then_some(w)
                })
                .min(),
            Node::Minor(m, i) => taken.contains(&Node::Major(m)).then_some(self.majors[m].ring[i].cost),
        }
    }

    /// A minor whose ring neighbour has been taken is closed for good.
    pub fn closed(&self, taken: &BTreeSet<Node>, n: Node) -> bool {
        match n {
            Node::Major(_) => false,
            Node::Minor(m, i) => {
                !taken.contains(&n) && (taken.contains(&Node::Minor(m, (i + 1) % RING)) || taken.contains(&Node::Minor(m, (i + RING - 1) % RING)))
            }
        }
    }
}

/// What a character has learned.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    pub taken: BTreeSet<Node>,
    pub points: u32,
}

impl Learned {
    /// Why a node can't be taken now, or its cost.
    pub fn check(&self, tree: &Tree, n: Node) -> Result<u32, &'static str> {
        if self.taken.contains(&n) {
            return Err("already learned");
        }
        if tree.closed(&self.taken, n) {
            return Err("closed by a neighbour");
        }
        let cost = tree.cost(&self.taken, n).ok_or("not reachable yet")?;
        if cost > self.points {
            return Err("not enough points");
        }
        Ok(cost)
    }

    pub fn take(&mut self, tree: &Tree, n: Node) -> Result<(), &'static str> {
        let cost = self.check(tree, n)?;
        self.points -= cost;
        self.taken.insert(n);
        Ok(())
    }

    pub fn perks(&self, tree: &Tree) -> Perks {
        let mut p = Perks::default();
        for n in &self.taken {
            for g in tree.gives(*n) {
                p.add(g);
            }
        }
        p
    }
}

/// Everything the taken nodes add up to.
#[derive(bevy::prelude::Resource, Debug, Clone, Default, PartialEq)]
pub struct Perks {
    pub carry: f32,
    pub air: f32,
    pub cold: f32,
    pub speed: f32,
    pub mining: f32,
    pub damage: f32,
    pub focus: f32,
    pub focus_regen: f32,
    pub levitate_cost: f32,
    pub spells: BTreeSet<Spell>,
}

impl Perks {
    fn add(&mut self, g: &Gives) {
        match *g {
            Gives::Carry(v) => self.carry += v,
            Gives::Air(v) => self.air += v,
            Gives::Cold(v) => self.cold += v,
            Gives::Speed(v) => self.speed += v,
            Gives::Mining(v) => self.mining += v,
            Gives::Damage(v) => self.damage += v,
            Gives::Focus(v) => self.focus += v,
            Gives::FocusRegen(v) => self.focus_regen += v,
            Gives::LevitateCost(v) => self.levitate_cost += v,
            Gives::Spell(s) => {
                self.spells.insert(s);
            }
        }
    }

    /// Thin air, made up for in part (water stays water).
    pub fn breathe(&self, air: f32) -> f32 {
        if air <= 0.0 { 0.0 } else { (air + (1.0 - air) * self.air.min(0.9)).min(1.0) }
    }

    /// Felt temperature with cold shrugged off (heat is untouched).
    pub fn feel(&self, feels: f32, comfortable: f32) -> f32 {
        if feels >= comfortable { feels } else { (feels + self.cold).min(comfortable) }
    }

    /// Carried weight as it feels with a bigger capacity.
    pub fn carried(&self, kg: f32, capacity: f32) -> f32 {
        kg * capacity / (capacity + self.carry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        Tree::from_ron(include_str!("../../../../assets/data/skills.ron")).unwrap()
    }

    #[test]
    fn root_first_then_links() {
        let t = tree();
        let mystic = t.majors.iter().position(|m| m.name == "mystic").unwrap();
        let mut l = Learned { points: 1, ..default_learned() };
        assert_eq!(l.check(&t, Node::Major(mystic)), Err("not reachable yet"));
        l.take(&t, Node::Major(t.root)).unwrap();
        assert_eq!(l.points, 0);
        assert_eq!(l.check(&t, Node::Major(mystic)), Err("not enough points"));
        l.points = 1;
        l.take(&t, Node::Major(mystic)).unwrap();
        // Levitation is the first spell, two points in.
        assert!(l.perks(&t).spells.contains(&Spell::Levitation));
    }

    #[test]
    fn a_minor_closes_its_neighbours() {
        let t = tree();
        let mut l = Learned { points: 20, ..default_learned() };
        let r = t.root;
        l.take(&t, Node::Major(r)).unwrap();
        l.take(&t, Node::Minor(r, 0)).unwrap();
        assert_eq!(l.check(&t, Node::Minor(r, 1)), Err("closed by a neighbour"));
        assert_eq!(l.check(&t, Node::Minor(r, 7)), Err("closed by a neighbour"));
        // Alternate nodes stay open: at most four of the eight.
        for i in [2, 4, 6] {
            l.take(&t, Node::Minor(r, i)).unwrap();
        }
        assert!((0..RING).all(|i| l.taken.contains(&Node::Minor(r, i)) || t.closed(&l.taken, Node::Minor(r, i))));
    }

    #[test]
    fn perks_add_up() {
        let t = tree();
        let mut l = Learned { points: 5, ..default_learned() };
        l.take(&t, Node::Major(t.root)).unwrap();
        l.take(&t, Node::Minor(t.root, 1)).unwrap();
        let p = l.perks(&t);
        assert_eq!(p.carry, 10.0);
        assert!((p.carried(40.0, 40.0) - 32.0).abs() < 1e-4);
        assert_eq!(p.feel(20.0, 8.0), 20.0);
        assert_eq!(p.breathe(0.0), 0.0);
    }

    #[test]
    fn every_ring_has_eight_and_every_major_is_reachable() {
        let t = tree();
        let mut l = Learned { points: 1000, ..default_learned() };
        // Take every reachable major until nothing changes.
        loop {
            let before = l.taken.len();
            for m in 0..t.majors.len() {
                let _ = l.take(&t, Node::Major(m));
            }
            if l.taken.len() == before {
                break;
            }
        }
        assert_eq!(l.taken.len(), t.majors.len());
    }

    fn default_learned() -> Learned {
        Learned::default()
    }
}
