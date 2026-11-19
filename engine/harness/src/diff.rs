use std::collections::BTreeSet;

use pumpkin_nbt::{NbtCompound, tag::NbtTag};
use serde::Serialize;

use crate::{
    scenario::{Allow, Position},
    snapshot::{Entity, World},
};

const EPSILON: f64 = 1e-6;
const IGNORED_KEYS: &[&str] = &[
    "UUID",
    "UUIDMost",
    "UUIDLeast",
    "LastUpdate",
    "InhabitedTime",
    "LastPlayed",
];

#[derive(Serialize)]
pub struct Difference {
    pub line: usize,
    pub check: String,
    pub position: Option<[f64; 3]>,
    pub field: String,
    pub reference: Option<String>,
    pub subject: Option<String>,
}

#[derive(Serialize)]
pub struct Suppressed {
    pub allow_line: usize,
    pub reason: String,
    pub difference: Difference,
}

#[derive(Default, Serialize)]
pub struct Report {
    pub scenario: String,
    pub directory: String,
    pub differences: Vec<Difference>,
    pub allowed: Vec<Suppressed>,
    pub error: Option<String>,
}

impl Report {
    pub fn add(&mut self, item: Difference, allows: &[Allow]) {
        if let Some(rule) = allows.iter().find(|rule| rule.pattern.matches(&item.field)) {
            self.allowed.push(Suppressed {
                allow_line: rule.line,
                reason: rule.reason.clone(),
                difference: item,
            });
        } else {
            self.differences.push(item);
        }
    }
}

pub struct Check<'a> {
    pub line: usize,
    pub name: String,
    pub report: &'a mut Report,
    pub allows: &'a [Allow],
}

impl Check<'_> {
    pub fn add(
        &mut self,
        position: Option<[f64; 3]>,
        field: String,
        reference: Option<String>,
        subject: Option<String>,
    ) {
        self.report.add(
            Difference {
                line: self.line,
                check: self.name.clone(),
                position,
                field,
                reference,
                subject,
            },
            self.allows,
        );
    }

    pub fn world(&mut self, mut reference: World, mut subject: World) {
        for pos in keys(&reference.blocks, &subject.blocks) {
            let a = reference
                .blocks
                .get(pos)
                .map_or("minecraft:air", String::as_str);
            let b = subject
                .blocks
                .get(pos)
                .map_or("minecraft:air", String::as_str);
            if a != b {
                self.add(
                    Some(coords(*pos)),
                    "block".into(),
                    Some(a.into()),
                    Some(b.into()),
                );
            }
        }
        for pos in keys(&reference.block_entities, &subject.block_entities) {
            let a = reference.block_entities.get(pos);
            let b = subject.block_entities.get(pos);
            match (a, b) {
                (Some(a), Some(b)) => self.compounds(Some(coords(*pos)), "block_entity", a, b),
                _ => self.add(
                    Some(coords(*pos)),
                    "block_entity".into(),
                    a.map(render_compound),
                    b.map(render_compound),
                ),
            }
        }
        sort_entities(&mut reference.entities);
        sort_entities(&mut subject.entities);
        self.entities(&reference.entities, &subject.entities);
    }

    fn entities(&mut self, reference: &[Entity], subject: &[Entity]) {
        let exact = match_entities(reference, subject, |a, b| compound_equal(&a.nbt, &b.nbt));
        let used_ref = exact.iter().flatten().copied().collect::<BTreeSet<_>>();
        let ref_left = reference
            .iter()
            .enumerate()
            .filter_map(|(i, e)| (!used_ref.contains(&i)).then_some(e))
            .collect::<Vec<_>>();
        let sub_left = subject
            .iter()
            .enumerate()
            .filter_map(|(i, e)| exact[i].is_none().then_some(e))
            .collect::<Vec<_>>();

        let paired = match_entities(&ref_left, &sub_left, |_, _| true);
        let used_ref = paired.iter().flatten().copied().collect::<BTreeSet<_>>();
        for (i, a) in ref_left.iter().enumerate() {
            if !used_ref.contains(&i) {
                self.add(
                    Some(a.position),
                    format!("entity.{}", a.kind),
                    Some(render_compound(&a.nbt)),
                    None,
                );
            }
        }
        for (b, a) in sub_left.into_iter().zip(paired) {
            let field = format!("entity.{}", b.kind);
            if let Some(i) = a {
                self.compounds(Some(ref_left[i].position), &field, &ref_left[i].nbt, &b.nbt);
            } else {
                self.add(Some(b.position), field, None, Some(render_compound(&b.nbt)));
            }
        }
    }

    pub fn compounds(
        &mut self,
        pos: Option<[f64; 3]>,
        field: &str,
        reference: &NbtCompound,
        subject: &NbtCompound,
    ) {
        visit_compound(field, reference, subject, &mut |field, a, b| {
            self.add(pos, field, a.map(render), b.map(render));
        });
    }
}

fn keys<'a, K: Ord, V>(
    a: &'a std::collections::BTreeMap<K, V>,
    b: &'a std::collections::BTreeMap<K, V>,
) -> BTreeSet<&'a K> {
    a.keys().chain(b.keys()).collect()
}

fn coords(pos: Position) -> [f64; 3] {
    pos.map(f64::from)
}

fn sort_entities(entities: &mut [Entity]) {
    entities.sort_by_cached_key(|entity| {
        (
            entity.kind.clone(),
            entity.position.map(f64::to_bits),
            render_compound(&entity.nbt),
        )
    });
}

fn match_entities<T: std::borrow::Borrow<Entity>>(
    a: &[T],
    b: &[T],
    equal_payload: impl Fn(&Entity, &Entity) -> bool,
) -> Vec<Option<usize>> {
    let candidates = a
        .iter()
        .map(|a| {
            let a = a.borrow();
            b.iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let b = b.borrow();
                    (a.kind == b.kind
                        && a.position
                            .into_iter()
                            .zip(b.position)
                            .all(|(a, b)| close(a, b))
                        && equal_payload(a, b))
                    .then_some(i)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut matched = vec![None; b.len()];
    let mut visited = vec![0; b.len()];
    for a in 0..candidates.len() {
        augment(a, a + 1, &candidates, &mut visited, &mut matched);
    }
    matched
}

fn augment(
    a: usize,
    round: usize,
    edges: &[Vec<usize>],
    seen: &mut [usize],
    matched: &mut [Option<usize>],
) -> bool {
    if let Some(&free) = edges[a]
        .iter()
        .find(|&&b| seen[b] != round && matched[b].is_none())
    {
        matched[free] = Some(a);
        return true;
    }
    for &b in &edges[a] {
        if seen[b] == round {
            continue;
        }
        seen[b] = round;
        if matched[b].is_none_or(|prior| augment(prior, round, edges, seen, matched)) {
            matched[b] = Some(a);
            return true;
        }
    }
    false
}

fn compound_equal(a: &NbtCompound, b: &NbtCompound) -> bool {
    a.child_tags
        .iter()
        .filter(|(key, _)| !ignored(key))
        .all(|(key, value)| b.get(key).is_some_and(|other| equal(value, other)))
        && b.child_tags
            .keys()
            .all(|key| ignored(key) || a.child_tags.contains_key(key))
}

fn visit_compound(
    path: &str,
    a: &NbtCompound,
    b: &NbtCompound,
    mismatch: &mut impl FnMut(String, Option<&NbtTag>, Option<&NbtTag>),
) {
    let keys: BTreeSet<_> = a
        .child_tags
        .keys()
        .chain(b.child_tags.keys())
        .filter(|key| !ignored(key))
        .collect();
    for key in keys {
        visit_tag(format!("{path}.{key}"), a.get(key), b.get(key), mismatch);
    }
}

fn visit_tag(
    path: String,
    a: Option<&NbtTag>,
    b: Option<&NbtTag>,
    mismatch: &mut impl FnMut(String, Option<&NbtTag>, Option<&NbtTag>),
) {
    match (a, b) {
        (Some(NbtTag::Compound(a)), Some(NbtTag::Compound(b))) => {
            visit_compound(&path, a, b, mismatch);
        }
        (Some(NbtTag::List(a)), Some(NbtTag::List(b))) => {
            for i in 0..a.len().max(b.len()) {
                visit_tag(format!("{path}[{i}]"), a.get(i), b.get(i), mismatch);
            }
        }
        (Some(a), Some(b)) if equal(a, b) => {}
        (None, None) => {}
        _ => mismatch(path, a, b),
    }
}

fn equal(a: &NbtTag, b: &NbtTag) -> bool {
    match (a, b) {
        (NbtTag::Compound(a), NbtTag::Compound(b)) => compound_equal(a, b),
        (NbtTag::List(a), NbtTag::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
        }
        (NbtTag::Double(a), NbtTag::Double(b)) => close(*a, *b),
        (NbtTag::Float(a), NbtTag::Float(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

fn close(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a - b).abs() <= EPSILON
}

fn ignored(key: &str) -> bool {
    IGNORED_KEYS.contains(&key)
}

fn render_compound(compound: &NbtCompound) -> String {
    let mut fields = compound
        .child_tags
        .iter()
        .filter(|(key, _)| !ignored(key))
        .collect::<Vec<_>>();
    fields.sort_by_key(|(key, _)| *key);
    format!(
        "{{{}}}",
        fields
            .into_iter()
            .map(|(key, value)| format!("{key}:{}", render(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render(tag: &NbtTag) -> String {
    match tag {
        NbtTag::Compound(compound) => render_compound(compound),
        NbtTag::List(list) => format!(
            "[{}]",
            list.iter().map(render).collect::<Vec<_>>().join(",")
        ),
        _ => tag.to_string(),
    }
}
