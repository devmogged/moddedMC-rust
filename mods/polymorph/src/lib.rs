//! Native Polymorph choice selection for the stock-client crafting menu.
//!
//! Right-click the visible recipe result to cycle through conflicting recipes
//! in either stock-client crafting menu without changing the ingredient grid.
//! The `/polymorph list` and `/polymorph choose N` commands remain available
//! for exact identities and automation-friendly testing. The Pumpkin result
//! slot pins this choice to the exact ingredient
//! snapshot and uses the same selected recipe for output and remainders.
//! This is intentionally not a fake replacement recipe or an extra item grant.

use std::sync::OnceLock;

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    commands::{self, Argument, Commands, Node},
    crafting::{self, Crafting, MAX_CRAFTING_CHOICES, RecipeChoices, RecipeMatch},
};
use serde::Deserialize;

const LIST: u64 = 1;
const CHOOSE: u64 = 2;
static CRAFTING: OnceLock<Crafting> = OnceLock::new();
static COMMANDS: OnceLock<Commands> = OnceLock::new();

#[derive(Deserialize)]
struct SavedItemId {
    id: String,
}

fn result_label(recipe: &RecipeMatch) -> String {
    let item = fastnbt::from_bytes::<SavedItemId>(&recipe.result)
        .ok()
        .map(|saved| saved.id)
        .unwrap_or_else(|| "<unknown result>".into());
    match &recipe.id {
        Some(id) => format!("{item} ({id})"),
        None => format!("{item} ({})", recipe.serializer),
    }
}

fn describe_choices(choices: &RecipeChoices) -> Vec<String> {
    let mut text = Vec::new();
    if choices.matches.is_empty() {
        text.push("No crafting recipe matches the current grid.".into());
        return text;
    }
    text.push(format!(
        "{} matching recipe(s). Use /polymorph choose <number> to select:",
        choices.matches.len()
    ));
    for (index, recipe) in choices.matches.iter().enumerate() {
        text.push(format!("{}. {}", index + 1, result_label(recipe)));
    }
    if choices.truncated {
        text.push("More choices exist; selection is limited to the first 64.".into());
    }
    text
}

struct Polymorph;

impl Mod for Polymorph {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let crafting = crafting::require()?;
        let commands = commands::require()?;
        if !crafting.supports_player_selection() {
            return Err(InterfaceUnavailable {
                name: "moddedmc:crafting.select_recipe",
                version: 1,
            });
        }
        if !crafting.enable_result_cycle() {
            return Err(InterfaceUnavailable {
                name: "moddedmc:crafting.enable_result_cycle",
                version: 1,
            });
        }
        let _ = CRAFTING.set(crafting);
        let _ = COMMANDS.set(commands);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let commands = COMMANDS.get().ok_or(RegistrationError::InvalidDefinition)?;
        commands.add(
            registry,
            "polymorph",
            "Choose among genuinely matching recipes in the current crafting grid",
            0,
            &[
                Node {
                    parent: None,
                    name: "list",
                    argument: Argument::Literal,
                    tag: Some(LIST),
                },
                Node {
                    parent: None,
                    name: "choose",
                    argument: Argument::Literal,
                    tag: None,
                },
                Node {
                    parent: Some(1),
                    name: "number",
                    argument: Argument::Integer {
                        min: 1,
                        max: i32::from(MAX_CRAFTING_CHOICES),
                    },
                    tag: Some(CHOOSE),
                },
            ],
        )
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let (Some(player), Some(&crafting)) = (command.player, CRAFTING.get()) else {
            command.reply("Open a player inventory or crafting table to use Polymorph.");
            return 0;
        };
        match tag {
            LIST => {
                let Some(choices) = crafting.current_choices(player, MAX_CRAFTING_CHOICES) else {
                    command.reply("No supported crafting menu is open.");
                    return 0;
                };
                for text in describe_choices(&choices) {
                    command.reply(&text);
                }
                1
            }
            CHOOSE => {
                let Some(number) = COMMANDS
                    .get()
                    .and_then(|commands| commands.integer(&command, "number"))
                else {
                    return 0;
                };
                let Some(index) = u16::try_from(number - 1).ok() else {
                    return 0;
                };
                if crafting.select_recipe(player, index) {
                    command.reply(&format!("Selected crafting recipe {}.", number));
                    1
                } else {
                    command.reply("That recipe no longer matches this crafting grid.");
                    0
                }
            }
            _ => 0,
        }
    }
}

moddedmc_api::export_mod!(Polymorph);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_result_is_reported_without_fabricating_recipe_ids() {
        let stack = fastnbt::to_bytes(&SavedItemIdForTest { id: "minecraft:stick" }).unwrap();
        let choice = RecipeMatch {
            result: stack,
            id: None,
            serializer: "minecraft:crafting_shapeless".into(),
        };
        assert_eq!(
            result_label(&choice),
            "minecraft:stick (minecraft:crafting_shapeless)"
        );
    }

    #[derive(serde::Serialize)]
    struct SavedItemIdForTest<'a> { id: &'a str }
}
