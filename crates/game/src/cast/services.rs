//! What the cast do for you, from `assets/data/services.ron` and `trades.ron`.
//!
//! A dialogue reply with `service: "..."` (or one added here from the data) asks
//! for one; `serve` does it between picking the reply and the next node, and
//! picks that node:
//!
//! - `weather` (Orrin): the weather now, when it clears and what comes next
//!   (`WeatherState.next` and `until`) -> `weather` / `weather_still`.
//! - `feed` (Messler): takes one food (what you hold, else the first in your
//!   pockets) and teaches the next recipe of `messler_teaches` -> `fed` / `fed_all` / `no_food`.
//! - `commission:<n>` (Gilm, Liora): hand over `needs`, get `makes` ->
//!   `commission_done` / `commission_short`. Gilm only offers what you can't make yet.
//! - `trade:<id>` (Wren): today's stock -> `trade_done` / `trade_short`.
//!
//! Messler also remarks on what happens at the cooking pot (`board::Cooked`).

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;
use serde::Deserialize;

use crate::clock::WorldClock;
use crate::interact::Wielded;
use crate::inventory::{grid::Stack, Inventory, Spill};
use crate::items::{ItemKind, Items};
use crate::npc::dialogue::Choice;
use crate::npc::talk::{Talk, TalkInput, TalkRun};
use crate::npc::{Npc, NpcBook};
use crate::player::Player;
use crate::station::board::{Benches, Cooked};
use crate::station::Urns;
use crate::survival::Toast;
use crate::weather::{WeatherKinds, WeatherState};

pub struct ServicesPlugin;

impl Plugin for ServicesPlugin {
    fn build(&self, app: &mut App) {
        let data: Services = ron::from_str(include_str!("../../../../assets/data/services.ron")).unwrap_or_else(|e| panic!("services.ron: {e}"));
        let trades: Trades = ron::from_str(include_str!("../../../../assets/data/trades.ron")).unwrap_or_else(|e| panic!("trades.ron: {e}"));
        app.insert_resource(data)
            .insert_resource(trades)
            .add_systems(crate::menu::GameStartup, add_choices)
            .add_systems(crate::menu::GameUpdate, (offers, serve).chain().after(TalkInput).before(TalkRun))
            .add_systems(crate::menu::GameUpdate, messler_remarks);
    }
}

#[derive(Resource, Deserialize, Debug, Clone)]
pub struct Services {
    pub messler_teaches: Vec<String>,
    pub messler_hearing: f32,
    /// By dialogue tree key.
    pub commissions: HashMap<String, Commissions>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Commissions {
    /// Offered only while no recipe you know makes it.
    #[serde(default)]
    pub only_unknown: bool,
    pub list: Vec<Commission>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Commission {
    pub makes: String,
    pub needs: BTreeMap<String, u32>,
}

#[derive(Resource, Deserialize, Debug, Clone)]
pub struct Trades {
    pub per_visit: usize,
    pub trades: Vec<Trade>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Trade {
    pub id: String,
    pub gives: (String, u32),
    pub wants: BTreeMap<String, u32>,
}

// ---- Pure rules ----

/// "in about 3 hours" (game hours left).
pub fn clears_in(hours: f64) -> String {
    if hours < 1.0 {
        "within the hour".into()
    } else {
        let h = hours.round() as i64;
        format!("in about {h} hour{}", if h == 1 { "" } else { "s" })
    }
}

/// Orrin's reading: the node and its `{now}`, `{clears}`, `{next}`.
pub fn reading(now: &str, next: Option<&str>, days_left: Option<f64>) -> (&'static str, HashMap<String, String>) {
    let mut vars = HashMap::from([("now".to_string(), capital(now))]);
    match (next, days_left) {
        (Some(next), Some(left)) => {
            vars.insert("clears".into(), clears_in(left.max(0.0) * 24.0));
            vars.insert("next".into(), next.to_lowercase());
            ("weather", vars)
        }
        _ => ("weather_still", vars),
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// The next lesson Messler hasn't taught you yet.
pub fn next_lesson<'a>(list: &'a [String], taught: &std::collections::BTreeSet<String>) -> Option<&'a str> {
    list.iter().find(|k| !taught.contains(*k)).map(|s| s.as_str())
}

/// What's missing of `needs`, given how many you have of each.
pub fn missing(needs: &BTreeMap<String, u32>, have: impl Fn(&str) -> u32) -> Vec<(String, u32)> {
    needs.iter().filter(|&(n, &want)| have(n) < want).map(|(n, &want)| (n.clone(), want - have(n))).collect()
}

/// Which trades are in stock on a day (indices, `per` of `n`, picked by the day).
pub fn stock(day: u32, per: usize, n: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut h = (day as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5EED_0F57;
    for i in (1..n).rev() {
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        order.swap(i, (h % (i as u64 + 1)) as usize);
    }
    order.truncate(per.min(n));
    order.sort_unstable();
    order
}

fn list(items: &Items, things: &BTreeMap<String, u32>) -> String {
    things.iter().map(|(n, c)| format!("{c} {}", items.id(n).map_or(n.as_str(), |i| items.get(i).title.as_str()))).collect::<Vec<_>>().join(", ")
}

fn plain_choice(text: String, service: String, needs: Vec<String>, without: Vec<String>) -> Choice {
    Choice { text, tone: None, goto: None, end: false, act: None, once: false, needs, without, sets: Vec::new(), service: Some(service) }
}

// ---- Systems ----

/// The commission and trade replies, from the data.
fn add_choices(mut book: ResMut<NpcBook>, data: Res<Services>, trades: Res<Trades>, items: Res<Items>) {
    let mut add = |tree: &str, node: &str, made: Vec<Choice>| {
        let Some(n) = book.trees.get_mut(tree).and_then(|t| t.nodes.get_mut(node)) else {
            warn!("dialogue.ron: no node `{node}` in `{tree}` for its services");
            return;
        };
        // Before the last reply ("Never mind").
        let at = n.choices.len().saturating_sub(1);
        n.choices.splice(at..at, made);
    };
    for (tree, c) in &data.commissions {
        let made = c
            .list
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let title = items.id(&m.makes).map_or(m.makes.clone(), |id| items.get(id).title.clone());
                let without = if c.only_unknown { vec![format!("knows:{}", m.makes)] } else { Vec::new() };
                plain_choice(format!("{title} ({})", list(&items, &m.needs)), format!("commission:{i}"), Vec::new(), without)
            })
            .collect();
        add(tree, "commission", made);
    }
    let made = trades
        .trades
        .iter()
        .map(|t| {
            let (n, c) = &t.gives;
            let gives = list(&items, &BTreeMap::from([(n.clone(), *c)]));
            plain_choice(format!("{gives} for {}", list(&items, &t.wants)), format!("trade:{}", t.id), vec![format!("stock:{}", t.id)], Vec::new())
        })
        .collect();
    add("wren", "trade", made);
}

/// While you talk: which replies count (what you can already make, today's stock).
fn offers(mut talk: ResMut<Talk>, book: Res<NpcBook>, npcs: Query<&Npc>, (data, trades): (Res<Services>, Res<Trades>), benches: Res<Benches>, items: Res<Items>, clock: Res<WorldClock>) {
    let Some(npc) = talk.with().and_then(|e| npcs.get(e).ok()) else { return };
    let Some(tree) = book.kinds.get(npc.kind).map(|k| k.dialogue.clone()) else { return };
    let mut extra = std::collections::HashSet::new();
    if let Some(c) = data.commissions.get(&tree).filter(|c| c.only_unknown) {
        for m in &c.list {
            let Some(id) = items.id(&m.makes) else { continue };
            if benches.0.recipes.iter().any(|r| r.makes.item == id && benches.0.knows(r)) {
                extra.insert(format!("knows:{}", m.makes));
            }
        }
    }
    if tree == "wren" {
        for i in stock(clock.day, trades.per_visit, trades.trades.len()) {
            extra.insert(format!("stock:{}", trades.trades[i].id));
        }
    }
    if talk.extra != extra {
        talk.extra = extra;
    }
}

/// Does what a reply asked for and picks the node that follows.
#[allow(clippy::too_many_arguments)]
fn serve(
    mut talk: ResMut<Talk>,
    book: Res<NpcBook>,
    npcs: Query<&Npc>,
    (data, trades, items): (Res<Services>, Res<Trades>, Res<Items>),
    (mut inv, wielded, benches, mut urns): (ResMut<Inventory>, Res<Wielded>, Res<Benches>, ResMut<Urns>),
    (weather, kinds, clock): (Res<WeatherState>, Res<WeatherKinds>, Res<WorldClock>),
    player: Res<Player>,
    (mut spill, mut toast): (MessageWriter<Spill>, MessageWriter<Toast>),
) {
    let Some(service) = talk.take_service() else { return };
    let Some(npc) = talk.with().and_then(|e| npcs.get(e).ok()) else { return };
    let Some(tree) = book.kinds.get(npc.kind).map(|k| k.dialogue.clone()) else { return };
    let title = |n: &str| items.id(n).map_or(n.to_string(), |i| items.get(i).title.clone());
    let have = |inv: &Inventory, n: &str| items.id(n).map_or(0, |i| inv.count(i));
    let mut give = |inv: &mut Inventory, name: &str, count: u32| {
        let Some(id) = items.id(name) else { return };
        if let Some(left) = inv.add(&items, Stack::new(id, count as u16)) {
            spill.write(Spill { stack: left, at: player.pos + Vec3::Y });
        }
    };
    let (node, vars): (&str, HashMap<String, String>) = match service.split_once(':').unwrap_or((service.as_str(), "")) {
        ("weather", _) => {
            let name = |i: usize| kinds.list.get(i).map_or("clear skies".to_string(), |k| k.title.clone());
            let left = weather.until.map(|u| u - clock.total_days());
            reading(&name(weather.current), weather.next.map(name).as_deref(), left)
        }
        ("feed", _) => {
            let is_food = |i: crate::items::ItemId| matches!(items.get(i).kind, ItemKind::Food { .. });
            let food = wielded.0.filter(|i| is_food(*i) && inv.count(*i) > 0).or_else(|| inv.pockets.items.iter().map(|p| p.stack.item).find(|i| is_food(*i)));
            match food {
                None => ("no_food", HashMap::new()),
                Some(f) => {
                    inv.take(f, 1);
                    match next_lesson(&data.messler_teaches, &urns.taught).map(str::to_string) {
                        None => ("fed_all", HashMap::new()),
                        Some(key) => {
                            urns.taught.insert(key.clone());
                            let recipe = benches.0.recipes.iter().find(|r| r.unlock.as_deref() == Some(key.as_str())).map_or(key.clone(), |r| items.get(r.makes.item).title.to_lowercase());
                            toast.write(Toast(format!("Messler taught you a recipe: {recipe}")));
                            ("fed", HashMap::from([("recipe".to_string(), recipe)]))
                        }
                    }
                }
            }
        }
        ("commission", i) => {
            let Some(m) = data.commissions.get(&tree).and_then(|c| c.list.get(i.parse::<usize>().unwrap_or(usize::MAX))) else { return };
            let short = missing(&m.needs, |n| have(&inv, n));
            if short.is_empty() {
                for (n, c) in &m.needs {
                    if let Some(id) = items.id(n) {
                        inv.take(id, *c);
                    }
                }
                give(&mut inv, &m.makes, 1);
                ("commission_done", HashMap::from([("made".to_string(), title(&m.makes))]))
            } else {
                ("commission_short", HashMap::from([("missing".to_string(), list(&items, &short.into_iter().collect()))]))
            }
        }
        ("trade", id) => {
            let Some(t) = trades.trades.iter().find(|t| t.id == id) else { return };
            let short = missing(&t.wants, |n| have(&inv, n));
            if short.is_empty() {
                for (n, c) in &t.wants {
                    if let Some(id) = items.id(n) {
                        inv.take(id, *c);
                    }
                }
                give(&mut inv, &t.gives.0, t.gives.1);
                ("trade_done", HashMap::from([("got".to_string(), list(&items, &BTreeMap::from([t.gives.clone()])))]))
            } else {
                ("trade_short", HashMap::from([("missing".to_string(), list(&items, &short.into_iter().collect()))]))
            }
        }
        _ => {
            warn!("no service `{service}`");
            return;
        }
    };
    talk.vars = vars;
    talk.go_to(node);
}

/// Messler, near you, has something to say about your cooking (his barks in lines.ron).
fn messler_remarks(
    mut cooked: MessageReader<Cooked>,
    data: Res<Services>,
    book: Res<NpcBook>,
    npcs: Query<(&Transform, &Npc)>,
    player: Res<Player>,
    mut bark: MessageWriter<crate::lines::Bark>,
) {
    for c in cooked.read() {
        let Some((tf, _)) = npcs.iter().find(|(tf, n)| {
            book.kinds.get(n.kind).is_some_and(|k| k.dialogue == "messler") && tf.translation.distance(player.pos) <= data.messler_hearing
        }) else {
            continue;
        };
        let trigger = match c {
            Cooked::Made => "recipe_made",
            Cooked::Failed => "recipe_failed",
            Cooked::Burnt => "food_burnt",
        };
        bark.write(crate::lines::Bark { speaker: "messler".into(), trigger: trigger.into(), at: Some(tf.translation + Vec3::Y) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orrin_reads_the_weather() {
        let (node, vars) = reading("Rain", Some("Fog"), Some(0.125));
        assert_eq!(node, "weather");
        assert_eq!(vars["now"], "Rain");
        assert_eq!(vars["clears"], "in about 3 hours");
        assert_eq!(vars["next"], "fog");
        assert_eq!(clears_in(0.4), "within the hour");
        assert_eq!(clears_in(1.2), "in about 1 hour");
        // Held weather (no next) has no end in sight.
        assert_eq!(reading("snow", None, None).0, "weather_still");
        assert_eq!(reading("snow", None, None).1["now"], "Snow");
    }

    #[test]
    fn messler_teaches_in_order_then_stops() {
        let list = vec!["messler:a".to_string(), "messler:b".to_string()];
        let mut taught = std::collections::BTreeSet::new();
        assert_eq!(next_lesson(&list, &taught), Some("messler:a"));
        taught.insert("messler:a".to_string());
        assert_eq!(next_lesson(&list, &taught), Some("messler:b"));
        taught.insert("messler:b".to_string());
        assert_eq!(next_lesson(&list, &taught), None);
    }

    #[test]
    fn commissions_need_everything() {
        let needs = BTreeMap::from([("raw_iron".to_string(), 4), ("coal".to_string(), 3)]);
        let have = |n: &str| if n == "raw_iron" { 5 } else { 1 };
        assert_eq!(missing(&needs, have), vec![("coal".to_string(), 2)]);
        assert!(missing(&needs, |_| 9).is_empty());
    }

    #[test]
    fn stock_rotates_by_day() {
        let a = stock(2, 4, 12);
        assert_eq!(a.len(), 4);
        assert_eq!(a, stock(2, 4, 12), "same day, same stock");
        assert!((0..20).map(|d| stock(d * 2, 4, 12)).any(|s| s != a), "it changes from visit to visit");
        assert_eq!(stock(5, 9, 3).len(), 3);
    }

    #[test]
    fn the_data_names_real_things_and_nodes() {
        let (_, items) = crate::items::tests::load();
        let data: Services = ron::from_str(include_str!("../../../../assets/data/services.ron")).expect("services.ron");
        let trades: Trades = ron::from_str(include_str!("../../../../assets/data/trades.ron")).expect("trades.ron");
        let trees = crate::npc::dialogue::load(include_str!("../../../../assets/data/dialogue.ron")).expect("dialogue.ron");
        let benches = include_str!("../../../../assets/data/benches.ron");
        for key in &data.messler_teaches {
            assert!(benches.contains(&format!("unlock: \"{key}\"")), "no recipe taught by {key}");
        }
        for (tree, c) in &data.commissions {
            let t = &trees[tree];
            for n in ["commission", "commission_done", "commission_short"] {
                assert!(t.nodes.contains_key(n), "{tree} has no {n}");
            }
            for m in &c.list {
                assert!(items.id(&m.makes).is_some(), "{}", m.makes);
                assert!(m.needs.keys().all(|n| items.id(n).is_some()), "{:?}", m.needs);
            }
        }
        for t in &trades.trades {
            assert!(items.id(&t.gives.0).is_some(), "{}", t.gives.0);
            assert!(t.wants.keys().all(|n| items.id(n).is_some()), "{:?}", t.wants);
        }
        for n in ["trade", "trade_done", "trade_short"] {
            assert!(trees["wren"].nodes.contains_key(n));
        }
        for n in ["fed", "fed_all", "no_food"] {
            assert!(trees["messler"].nodes.contains_key(n));
        }
        for n in ["weather", "weather_still"] {
            assert!(trees["orrin"].nodes.contains_key(n));
        }
    }
}
