//! `IvyTradeReloadListener`: all datapack trade sources in resource-id order.

use fastnbt::Value;
use serde_json::Value as Json;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Mutex, PoisonError},
};

use crate::{RESOURCES, ai::random};

static SOURCES: Mutex<(u64, u64, Vec<Json>)> = Mutex::new((0, 0, Vec::new()));

pub fn revision() -> Option<u64> {
    let mut sources = SOURCES.lock().unwrap_or_else(PoisonError::into_inner);
    refresh(&mut sources)?;
    Some(sources.1)
}

fn refresh(sources: &mut (u64, u64, Vec<Json>)) -> Option<()> {
    let resources = RESOURCES.get()?;
    let generation = resources.generation();
    if generation == 0 {
        return None;
    }
    if sources.0 != generation {
        let mut ids = resources.list("ivy_trades")?;
        ids.sort_unstable();
        let mut fingerprint = DefaultHasher::new();
        generation.hash(&mut fingerprint);
        let roots = ids
            .into_iter()
            .filter(|id| id.ends_with(".json"))
            .filter_map(|id| {
                let bytes = resources.read(&id);
                id.hash(&mut fingerprint);
                bytes.hash(&mut fingerprint);
                let parsed = bytes
                    .and_then(|bytes| serde_json::from_slice::<Json>(&bytes).ok())
                    .filter(valid_source);
                if parsed.is_none() {
                    moddedmc_api::warn(&format!("Saint's Dragons: invalid Ivy trade file {id}"));
                }
                parsed
            })
            .collect();
        *sources = (generation, fingerprint.finish(), roots);
    }
    Some(())
}

/// Roll a fresh set of offers after a reload or restock.
pub fn roll() -> Option<Vec<u8>> {
    let mut sources = SOURCES.lock().unwrap_or_else(PoisonError::into_inner);
    refresh(&mut sources)?;
    let recipes = sources
        .2
        .iter()
        .filter_map(|root| roll_from(root, &mut random))
        .flatten()
        .collect();
    fastnbt::to_bytes(&Value::Compound(
        [("Recipes".to_owned(), Value::List(recipes))].into(),
    ))
    .ok()
}

fn valid_source(root: &Json) -> bool {
    let valid_trade = |trade: &Json| {
        trade.get("results").is_none_or(|results| {
            results.as_array().is_some_and(|entries| {
                !entries.is_empty()
                    && entries
                        .iter()
                        .all(|entry| valid_weight(entry) && stack(entry, &mut |_| 0).is_some())
            })
        }) && offer(trade, &mut |_| 0).is_some()
    };
    let trades = root.get("trades").map(Json::as_array);
    let pools = root.get("pools").map(Json::as_array);
    if trades == Some(None) || pools == Some(None) {
        return false;
    }
    let trades = trades.flatten().map_or(&[][..], Vec::as_slice);
    let pools = pools.flatten().map_or(&[][..], Vec::as_slice);
    (!trades.is_empty() || !pools.is_empty())
        && trades.iter().all(valid_trade)
        && pools.iter().all(|pool| {
            pool.get("entries")
                .and_then(Json::as_array)
                .is_some_and(|entries| {
                    let rolls = pool.get("rolls").map_or(Some(1), Json::as_u64);
                    rolls.is_some_and(|rolls| rolls > 0 && rolls <= entries.len() as u64)
                        && entries.iter().all(|entry| {
                            valid_weight(entry) && valid_trade(entry.get("trade").unwrap_or(entry))
                        })
                })
        })
}

fn valid_weight(entry: &Json) -> bool {
    entry.get("weight").is_none_or(|weight| {
        weight
            .as_u64()
            .is_some_and(|w| (1..=i32::MAX as u64).contains(&w))
    })
}

fn roll_from(root: &Json, rng: &mut dyn FnMut(u64) -> u64) -> Option<Vec<Value>> {
    let mut recipes = Vec::new();
    for trade in root
        .get("trades")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        recipes.push(offer(trade, rng)?);
    }
    for pool in root
        .get("pools")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let rolls = pool.get("rolls").and_then(Json::as_u64).unwrap_or(1);
        let mut entries: Vec<&Json> = pool.get("entries")?.as_array()?.iter().collect();
        for _ in 0..rolls {
            let weight = |entry: &Json| entry.get("weight").and_then(Json::as_u64).unwrap_or(1);
            let total: u64 = entries.iter().map(|entry| weight(entry)).sum();
            if total == 0 {
                break;
            }
            let mut left = rng(total);
            let index = entries
                .iter()
                .position(|entry| {
                    let w = weight(entry);
                    let hit = left < w;
                    left = left.saturating_sub(w);
                    hit
                })
                .unwrap_or(entries.len() - 1);
            let entry = entries.remove(index);
            recipes.push(offer(entry.get("trade").unwrap_or(entry), rng)?);
        }
    }
    Some(recipes)
}

fn offer(trade: &Json, rng: &mut dyn FnMut(u64) -> u64) -> Option<Value> {
    let mut recipe = vec![("buy".to_owned(), stack(trade.get("cost_a")?, rng)?)];
    if let Some(cost_b) = trade.get("cost_b") {
        recipe.push(("buyB".to_owned(), stack(cost_b, rng)?));
    }
    let sell = match trade.get("results").and_then(Json::as_array) {
        Some(results) => {
            let weight = |entry: &Json| entry.get("weight").and_then(Json::as_u64).unwrap_or(1);
            let mut left = rng(results.iter().map(weight).sum::<u64>().max(1));
            let pick = results
                .iter()
                .find(|entry| {
                    let w = weight(entry);
                    let hit = left < w;
                    left = left.saturating_sub(w);
                    hit
                })
                .or(results.last())?;
            stack(pick, rng)?
        }
        None => stack(trade.get("result")?, rng)?,
    };
    let int = |key: &str, default: i64| trade.get(key).and_then(Json::as_i64).unwrap_or(default);
    recipe.extend([
        ("sell".to_owned(), sell),
        ("maxUses".to_owned(), Value::Int(int("max_uses", 5) as i32)),
        ("uses".to_owned(), Value::Int(0)),
        // Ivy has no trade levels and `rewardTradeXp` does nothing.
        ("xp".to_owned(), Value::Int(0)),
        ("rewardExp".to_owned(), Value::Byte(0)),
        (
            "priceMultiplier".to_owned(),
            Value::Float(
                trade
                    .get("price_multiplier")
                    .and_then(Json::as_f64)
                    .unwrap_or(0.05) as f32,
            ),
        ),
        ("specialPrice".to_owned(), Value::Int(0)),
        ("demand".to_owned(), Value::Int(0)),
    ]);
    Some(Value::Compound(recipe.into_iter().collect()))
}

/// A saved item stack: the id, a count from a number or `{min, max}`, and enchantments that
/// each apply with their `chance`.
fn stack(spec: &Json, rng: &mut dyn FnMut(u64) -> u64) -> Option<Value> {
    let id = spec.get("item")?.as_str()?;
    let count = match spec.get("count") {
        None => 1,
        Some(Json::Object(range)) => {
            let min = range.get("min").and_then(Json::as_u64).unwrap_or(1);
            let max = range
                .get("max")
                .and_then(Json::as_u64)
                .unwrap_or(min)
                .max(min);
            if min == 0 || max > i32::MAX as u64 {
                return None;
            }
            min + rng(max - min + 1)
        }
        Some(count) => count.as_u64().filter(|count| *count > 0)?,
    };
    let mut item = vec![
        ("id".to_owned(), Value::String(id.to_owned())),
        ("count".to_owned(), Value::Int(i32::try_from(count).ok()?)),
    ];
    let levels: Vec<(String, Value)> = spec
        .get("enchantments")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter(|enchantment| {
            let chance = enchantment
                .get("chance")
                .and_then(Json::as_f64)
                .unwrap_or(1.0);
            (rng(1_000_000) as f64) < chance * 1_000_000.0
        })
        .filter_map(|enchantment| {
            Some((
                enchantment.get("id")?.as_str()?.to_owned(),
                Value::Int(enchantment.get("level").and_then(Json::as_i64).unwrap_or(1) as i32),
            ))
        })
        .collect();
    if !levels.is_empty() {
        item.push((
            "components".to_owned(),
            Value::Compound(
                [(
                    "minecraft:enchantments".to_owned(),
                    Value::Compound(levels.into_iter().collect()),
                )]
                .into(),
            ),
        ));
    }
    Some(Value::Compound(item.into_iter().collect()))
}

#[cfg(test)]
mod tests {
    use super::{roll_from, valid_source};
    use fastnbt::Value;

    #[test]
    fn invalid_unselected_result_rejects_entire_file() {
        let mut root = serde_json::json!({"trades": [{
            "cost_a": {"item": "minecraft:egg"},
            "results": [
                {"item": "minecraft:diamond"},
                {"item": "minecraft:coal", "weight": u64::MAX}
            ]
        }]});
        assert!(!valid_source(&root));
        root["trades"][0]["results"][1]["weight"] = serde_json::json!(1);
        assert!(valid_source(&root));
        root["trades"][0]["results"][1]["count"] = serde_json::json!(0);
        assert!(!valid_source(&root));
    }

    #[test]
    fn rolls_fixed_trades_and_distinct_pool_entries() {
        let root = serde_json::json!({
            "trades": [{"cost_a": {"item": "minecraft:egg", "count": 4},
                        "cost_b": {"item": "minecraft:salmon", "count": 4},
                        "result": {"item": "saintsdragons:hearty_dragon_meal", "count": 6},
                        "max_uses": 9999}],
            "pools": [{"rolls": 2, "entries": [
                {"trade": {"cost_a": {"item": "minecraft:dirt"},
                           "results": [{"item": "minecraft:coal", "count": {"min": 2, "max": 4}}]}},
                {"trade": {"cost_a": {"item": "minecraft:stone"},
                           "result": {"item": "minecraft:diamond_chestplate",
                                      "enchantments": [{"id": "minecraft:mending", "level": 1}]}}}
            ]}]
        });
        let mut next = 0;
        let recipes = roll_from(&root, &mut |bound| {
            next += 1;
            next % bound
        })
        .unwrap();
        assert_eq!(recipes.len(), 3);
        let buys: Vec<String> = recipes
            .iter()
            .map(|recipe| match recipe {
                Value::Compound(c) => match &c["buy"] {
                    Value::Compound(b) => format!("{:?}", b["id"]),
                    _ => String::new(),
                },
                _ => String::new(),
            })
            .collect();
        assert!(
            buys[1] != buys[2],
            "a pool doesn't roll the same entry twice"
        );
        let Value::Compound(fixed) = &recipes[0] else {
            panic!()
        };
        assert!(fixed.contains_key("buyB"));
        assert_eq!(fixed["maxUses"], Value::Int(9999));
    }
}
