//! The world-wide shrine list and who has discovered what. Plain data, no host calls.
//!
//! Saved as tab-separated lines:
//!   S <key> <name>      a shrine
//!   D <player> <key>    a discovery
//! where `key` is `dimension/x/y/z`.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub dimension: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Site {
    pub fn key(&self) -> String {
        format!("{}/{}/{}/{}", self.dimension, self.x, self.y, self.z)
    }

    fn parse(key: &str) -> Option<Self> {
        let mut parts = key.rsplitn(4, '/');
        let z = parts.next()?.parse().ok()?;
        let y = parts.next()?.parse().ok()?;
        let x = parts.next()?.parse().ok()?;
        let dimension = parts.next()?.to_owned();
        Some(Self { dimension, x, y, z })
    }
}

#[derive(Clone, Debug)]
pub struct Shrine {
    pub site: Site,
    pub name: String,
}

#[derive(Default)]
pub struct Shrines {
    by_key: BTreeMap<String, Shrine>,
    /// (player, shrine key)
    discovered: BTreeSet<(String, String)>,
}

impl Shrines {
    pub fn load(text: &str) -> Self {
        let mut shrines = Self::default();
        for line in text.lines() {
            match line.split('\t').collect::<Vec<_>>().as_slice() {
                ["S", key, name] => {
                    if let Some(site) = Site::parse(key) {
                        shrines.by_key.insert(
                            (*key).to_owned(),
                            Shrine {
                                site,
                                name: (*name).to_owned(),
                            },
                        );
                    }
                }
                ["D", player, key] => {
                    shrines
                        .discovered
                        .insert(((*player).to_owned(), (*key).to_owned()));
                }
                _ => {}
            }
        }
        shrines
            .discovered
            .retain(|(_, key)| shrines.by_key.contains_key(key));
        shrines
    }

    pub fn save(&self) -> String {
        let mut out = String::new();
        for (key, shrine) in &self.by_key {
            let _ = writeln!(out, "S\t{key}\t{}", shrine.name);
        }
        for (player, key) in &self.discovered {
            let _ = writeln!(out, "D\t{player}\t{key}");
        }
        out
    }

    /// Adds a shrine, discovered by whoever placed it. Returns its name.
    pub fn add(&mut self, site: Site, owner: String) -> String {
        let key = site.key();
        let name = format!("Wayshrine {} {} {}", site.x, site.y, site.z);
        self.by_key.insert(
            key.clone(),
            Shrine {
                site,
                name: name.clone(),
            },
        );
        if !owner.is_empty() {
            self.discovered.insert((owner, key));
        }
        name
    }

    pub fn remove(&mut self, site: &Site) -> bool {
        let key = site.key();
        self.discovered.retain(|(_, k)| *k != key);
        self.by_key.remove(&key).is_some()
    }

    pub fn get(&self, site: &Site) -> Option<&Shrine> {
        self.by_key.get(&site.key())
    }

    /// Returns true if this is the player's first visit to an existing shrine.
    pub fn discover(&mut self, player: &str, site: &Site) -> bool {
        let key = site.key();
        self.by_key.contains_key(&key) && self.discovered.insert((player.to_owned(), key))
    }

    /// Shrines the player can travel to from `here`.
    pub fn known_by(&self, player: &str, here: &Site) -> Vec<Shrine> {
        let here = here.key();
        self.discovered
            .iter()
            .filter(|(p, key)| p == player && *key != here)
            .filter_map(|(_, key)| self.by_key.get(key).cloned())
            .collect()
    }

    /// The shrine behind a menu choice, only if this player has discovered it.
    pub fn travel_target(&self, player: &str, key: &str) -> Option<Shrine> {
        self.discovered
            .contains(&(player.to_owned(), key.to_owned()))
            .then(|| self.by_key.get(key).cloned())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32) -> Site {
        Site {
            dimension: "minecraft:overworld".into(),
            x,
            y: 64,
            z: -3,
        }
    }

    #[test]
    fn discovery_travel_and_persistence() {
        let mut s = Shrines::default();
        s.add(at(0), "alice".into());
        s.add(at(100), String::new());
        assert!(s.known_by("alice", &at(0)).is_empty());
        assert!(
            s.travel_target("alice", &at(100).key()).is_none(),
            "not discovered yet"
        );

        assert!(s.discover("alice", &at(100)));
        assert!(!s.discover("alice", &at(100)), "second visit is not new");
        assert!(!s.discover("alice", &at(5)), "no shrine there");
        assert_eq!(s.known_by("alice", &at(0))[0].site, at(100));
        assert!(
            s.travel_target("bob", &at(100).key()).is_none(),
            "bob never visited"
        );

        let reloaded = Shrines::load(&s.save());
        assert_eq!(
            reloaded
                .travel_target("alice", &at(100).key())
                .map(|t| t.site),
            Some(at(100))
        );

        let mut s = reloaded;
        assert!(s.remove(&at(100)));
        assert!(s.travel_target("alice", &at(100).key()).is_none());
        assert!(
            Shrines::load(&s.save())
                .known_by("alice", &at(0))
                .is_empty()
        );
        assert_eq!(
            Site::parse("minecraft:the_nether/-1/2/-3"),
            Some(Site {
                dimension: "minecraft:the_nether".into(),
                x: -1,
                y: 2,
                z: -3
            })
        );
    }
}
