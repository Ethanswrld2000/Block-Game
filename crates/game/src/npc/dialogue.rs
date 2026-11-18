//! Pure dialogue trees (`assets/data/dialogue.ron`, one tree per NPC kind).
//!
//! A tree is a set of nodes. A node is a few lines the NPC says, a mood for
//! their face and hands, and the replies you can pick. A reply can lead to
//! another node, end the talk, or give the NPC an order (follow, stay,
//! wander). Flags remember what happened: a node or reply `sets` them, and a
//! reply that `needs` or is `without` a flag only shows when it fits. The NPC
//! keeps its flags between talks, so the second greeting can differ
//! (`again`). Replies you have already picked show dimmed, and `once`
//! replies go away after you pick them.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

/// The face and hands a node is said with.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mood {
    #[default]
    Neutral,
    Happy,
    Curious,
    Worried,
    Sad,
    Angry,
    Surprised,
}

/// What a reply tells the NPC to do.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Follow,
    Stay,
    Wander,
    /// Cast the spirit spell on you (the guides at dungeon gates).
    Spirit,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Choice {
    pub text: String,
    /// A short tag shown before the reply ("Kind", "Blunt", "Ask").
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default)]
    pub goto: Option<String>,
    #[serde(default)]
    pub end: bool,
    #[serde(default)]
    pub act: Option<Act>,
    #[serde(default)]
    pub once: bool,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub without: Vec<String>,
    #[serde(default)]
    pub sets: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Node {
    pub say: Vec<String>,
    #[serde(default)]
    pub mood: Mood,
    #[serde(default)]
    pub sets: Vec<String>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// With no choices: go straight on to this node (else the talk ends).
    #[serde(default)]
    pub then: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Tree {
    pub start: String,
    /// The opening once you've met (the flag `met` is set on the first talk).
    #[serde(default)]
    pub again: Option<String>,
    pub nodes: HashMap<String, Node>,
}

/// A reply on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// Index into the node's `choices`.
    pub index: usize,
    pub text: String,
    pub tone: Option<String>,
    /// Picked before: shown dimmed.
    pub asked: bool,
}

/// Where a reply leads.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub next: Option<String>,
    pub act: Option<Act>,
}

pub const MET: &str = "met";
/// Set while the NPC follows you (a `Follow` reply sets it, `Stay`/`Wander` clear it).
pub const FOLLOWING: &str = "following";

pub fn load(text: &str) -> Result<HashMap<String, Tree>, String> {
    // `goto: "node"` reads as `Some("node")`.
    let ron = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let trees: HashMap<String, Tree> = ron.from_str(text).map_err(|e| e.to_string())?;
    for (name, t) in &trees {
        t.check().map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(trees)
}

impl Tree {
    /// Every node it points at exists and every node says something.
    pub fn check(&self) -> Result<(), String> {
        let has = |id: &str| self.nodes.contains_key(id);
        if !has(&self.start) {
            return Err(format!("start node `{}` is missing", self.start));
        }
        if let Some(a) = &self.again
            && !has(a)
        {
            return Err(format!("again node `{a}` is missing"));
        }
        for (id, n) in &self.nodes {
            if n.say.is_empty() {
                return Err(format!("node `{id}` says nothing"));
            }
            if let Some(t) = &n.then
                && !has(t)
            {
                return Err(format!("node `{id}` goes on to missing `{t}`"));
            }
            for c in &n.choices {
                if let Some(g) = &c.goto
                    && !has(g)
                {
                    return Err(format!("reply `{}` in `{id}` goes to missing `{g}`", c.text));
                }
                if c.goto.is_none() && !c.end && c.act.is_none() {
                    return Err(format!("reply `{}` in `{id}` leads nowhere (give it goto, end or act)", c.text));
                }
            }
        }
        Ok(())
    }

    /// The node a talk opens on.
    pub fn opening(&self, flags: &HashSet<String>) -> &str {
        match &self.again {
            Some(a) if flags.contains(MET) => a,
            _ => &self.start,
        }
    }

    /// Entering a node sets its flags.
    pub fn enter(&self, id: &str, flags: &mut HashSet<String>) {
        flags.insert(MET.to_string());
        if let Some(n) = self.nodes.get(id) {
            flags.extend(n.sets.iter().cloned());
        }
    }

    /// The replies that show at a node.
    pub fn offers(&self, id: &str, flags: &HashSet<String>, asked: &HashSet<(String, usize)>) -> Vec<Offer> {
        let Some(n) = self.nodes.get(id) else { return Vec::new() };
        n.choices
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                let picked = asked.contains(&(id.to_string(), *i));
                c.needs.iter().all(|f| flags.contains(f)) && !c.without.iter().any(|f| flags.contains(f)) && !(c.once && picked)
            })
            .map(|(i, c)| Offer { index: i, text: c.text.clone(), tone: c.tone.clone(), asked: asked.contains(&(id.to_string(), i)) })
            .collect()
    }

    /// Pick reply `index` at node `id`.
    pub fn pick(&self, id: &str, index: usize, flags: &mut HashSet<String>, asked: &mut HashSet<(String, usize)>) -> Outcome {
        let Some(c) = self.nodes.get(id).and_then(|n| n.choices.get(index)) else {
            return Outcome { next: None, act: None };
        };
        asked.insert((id.to_string(), index));
        flags.extend(c.sets.iter().cloned());
        // Orders keep the `following` flag, so replies can depend on it.
        match c.act {
            Some(Act::Follow) => {
                flags.insert(FOLLOWING.to_string());
            }
            Some(Act::Stay | Act::Wander) => {
                flags.remove(FOLLOWING);
            }
            Some(Act::Spirit) | None => {}
        }
        let next = if c.end { None } else { c.goto.clone() };
        Outcome { next, act: c.act }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trees() -> HashMap<String, Tree> {
        load(include_str!("../../../../assets/data/dialogue.ron")).unwrap_or_else(|e| panic!("dialogue.ron: {e}"))
    }

    #[test]
    fn data_loads_and_every_link_resolves() {
        let t = trees();
        assert!(t.contains_key("mossa"));
    }

    #[test]
    fn npcs_ron_names_real_trees() {
        let t = trees();
        let kinds = super::super::data::load(include_str!("../../../../assets/data/npcs.ron")).expect("npcs.ron");
        for k in &kinds {
            assert!(t.contains_key(&k.dialogue), "{} has no tree `{}`", k.name, k.dialogue);
        }
    }

    #[test]
    fn second_meeting_opens_differently() {
        let t = &trees()["mossa"];
        let mut flags = HashSet::new();
        let first = t.opening(&flags).to_string();
        t.enter(&first, &mut flags);
        assert_ne!(t.opening(&flags), first);
    }

    #[test]
    fn replies_follow_flags_and_once() {
        let text = r#"{ "t": (start: "a", nodes: {
            "a": (say: ["hi"], choices: [
                (text: "one", goto: "a", once: true, sets: ["x"]),
                (text: "two", goto: "a", needs: ["x"]),
                (text: "three", end: true, without: ["x"]),
            ]),
        }) }"#;
        let t = &load(text).unwrap()["t"];
        let mut flags = HashSet::new();
        let mut asked = HashSet::new();
        let names = |f: &HashSet<String>, a: &HashSet<(String, usize)>| t.offers("a", f, a).into_iter().map(|o| o.text).collect::<Vec<_>>();
        assert_eq!(names(&flags, &asked), ["one", "three"]);
        let out = t.pick("a", 0, &mut flags, &mut asked);
        assert_eq!(out.next.as_deref(), Some("a"));
        assert_eq!(names(&flags, &asked), ["two"]);
        let out = t.pick("a", 1, &mut flags, &mut asked);
        assert!(t.offers("a", &flags, &asked)[0].asked, "picked before shows dimmed");
        assert_eq!(out.next.as_deref(), Some("a"));
    }

    #[test]
    fn broken_links_are_caught() {
        let text = r#"{ "t": (start: "a", nodes: { "a": (say: ["hi"], choices: [(text: "go", goto: "nowhere")]) }) }"#;
        assert!(load(text).is_err());
    }
}
