# Epic Knights 10.18 - source registration reconciliation

This audit is from local decompiled 10.18 Fabric bytecode, not the outdated buja 10.11 jar; no proprietary code or assets are checked in.

## Registry facts

- Literal item declaration calls: 145 (includes Fabric-only tin/silver/bronze ingots).
- Dynamically instantiated weapon families: 20.
- Dynamically instantiated shield families: 9.
- Material tiers instantiated per family: 11: wood, stone, iron, gold, diamond, netherite, copper, steel, silver, tin, bronze.
- Pre-gate inferred item definitions: 464. Actual registrations depend on config disabled flags. This is NOT a verified runtime count.
- Explicit registered blocks: 12, 11 pavise materials and an upper-collision helper.

## Explicit registered blocks

magistuarmory:wood_pavise, magistuarmory:gold_pavise, magistuarmory:stone_pavise, magistuarmory:iron_pavise, magistuarmory:diamond_pavise, magistuarmory:netherite_pavise, magistuarmory:tin_pavise, magistuarmory:copper_pavise, magistuarmory:silver_pavise, magistuarmory:bronze_pavise, magistuarmory:steel_pavise, magistuarmory:pavise_upper_collision

## Literal item definitions grouped by constructor


### addArmorDecorationItem (7)

magistuarmory:gazelle_horns_decoration, magistuarmory:duck_decoration, magistuarmory:bullhorns_decoration, magistuarmory:antlers_decoration, magistuarmory:demon_horns_decoration, magistuarmory:rondel_decoration, magistuarmory:cat_ears_decoration

### addDyeableArmorDecorationItem (19)

magistuarmory:torse_and_mantle_decoration, magistuarmory:spike_decoration, magistuarmory:horse_decoration, magistuarmory:unicorn_decoration, magistuarmory:dragon_decoration, magistuarmory:torse_decoration, magistuarmory:two_plumes_decoration, magistuarmory:big_plume_decoration, magistuarmory:plume_left_decoration, magistuarmory:plume_middle_decoration, magistuarmory:plume_right_decoration, magistuarmory:bear_decoration, magistuarmory:lily_decoration, magistuarmory:lion_decoration, magistuarmory:feathers_decoration, magistuarmory:viking_horns_decoration, magistuarmory:griffin_decoration, magistuarmory:hood_decoration, magistuarmory:ecranche_decoration

### addDyeableMedievalArmorItem (11)

magistuarmory:greathelm, magistuarmory:crusader_chestplate, magistuarmory:crusader_boots, magistuarmory:coif, magistuarmory:gambeson_chestplate, magistuarmory:pantyhose, magistuarmory:gambeson_boots, magistuarmory:brigandine_chestplate, magistuarmory:norman_helmet, magistuarmory:cuirassier_chestplate, magistuarmory:cuirassier_leggings

### addDyeableWearableArmorDecorationItem (2)

magistuarmory:crown_decoration, magistuarmory:flowercrown_decoration

### addIngredientItem (32)

magistuarmory:apostolic_cross_pattern, magistuarmory:bowl_pattern, magistuarmory:bull_pattern, magistuarmory:chess_pattern, magistuarmory:crusader_cross_pattern, magistuarmory:dragon_pattern, magistuarmory:eagle_pattern, magistuarmory:horse_pattern, magistuarmory:lily_pattern, magistuarmory:lion1_pattern, magistuarmory:lion2_pattern, magistuarmory:orthodox_cross_pattern, magistuarmory:snake_pattern, magistuarmory:sun_pattern, magistuarmory:swords_pattern, magistuarmory:tower_pattern, magistuarmory:tree_pattern, magistuarmory:two_headed_eagle_pattern, magistuarmory:steel_ingot, magistuarmory:steel_nugget, magistuarmory:steel_ring, magistuarmory:steel_chainmail, magistuarmory:steel_plate, magistuarmory:leather_strip, magistuarmory:hilt, magistuarmory:pole, magistuarmory:steel_chain, magistuarmory:woolen_fabric, magistuarmory:small_steel_plate, magistuarmory:lamellar_rows, magistuarmory:darkening_template, magistuarmory:gilding_template

### addJoustingItem (4)

magistuarmory:stechhelm, magistuarmory:jousting_chestplate, magistuarmory:jousting_leggings, magistuarmory:jousting_boots

### addKnightItem (2)

magistuarmory:armet, magistuarmory:ceremonialarmet

### addMedievalArmorItem (50)

magistuarmory:knight_chestplate, magistuarmory:knight_leggings, magistuarmory:knight_boots, magistuarmory:sallet, magistuarmory:gothic_chestplate, magistuarmory:gothic_leggings, magistuarmory:gothic_boots, magistuarmory:maximilian_helmet, magistuarmory:maximilian_chestplate, magistuarmory:maximilian_leggings, magistuarmory:maximilian_boots, magistuarmory:chainmail_helmet, magistuarmory:chainmail_chestplate, magistuarmory:chainmail_leggings, magistuarmory:chainmail_boots, magistuarmory:kettlehat, magistuarmory:platemail_chestplate, magistuarmory:platemail_leggings, magistuarmory:platemail_boots, magistuarmory:barbute, magistuarmory:halfarmor_chestplate, magistuarmory:crusader_leggings, magistuarmory:ceremonial_chestplate, magistuarmory:ceremonial_boots, magistuarmory:shishak, magistuarmory:rustedbarbute, magistuarmory:rustedhalfarmor_chestplate, magistuarmory:rustedgreathelm, magistuarmory:rustedcrusader_chestplate, magistuarmory:rustedcrusader_boots, magistuarmory:rustednorman_helmet, magistuarmory:rustedchainmail_helmet, magistuarmory:rustedchainmail_chestplate, magistuarmory:rustedchainmail_leggings, magistuarmory:rustedchainmail_boots, magistuarmory:rustedkettlehat, magistuarmory:bascinet, magistuarmory:xivcenturyknight_chestplate, magistuarmory:xivcenturyknight_leggings, magistuarmory:xivcenturyknight_boots, magistuarmory:wingedhussar_chestplate, magistuarmory:cuirassier_helmet, magistuarmory:cuirassier_boots, magistuarmory:grand_bascinet, magistuarmory:kastenbrust_chestplate, magistuarmory:kastenbrust_leggings, magistuarmory:kastenbrust_boots, magistuarmory:face_helmet, magistuarmory:lamellar_chestplate, magistuarmory:lamellar_boots

### addMedievalBowItem (1)

magistuarmory:longbow

### addMedievalCrossbowItem (1)

magistuarmory:heavy_crossbow

### addMedievalShieldItem (1)

magistuarmory:corruptedroundshield

### addMedievalWeaponItem (8)

magistuarmory:blacksmith_hammer, magistuarmory:barbedclub, magistuarmory:pitchfork, magistuarmory:noble_sword, magistuarmory:rusted_bastardsword, magistuarmory:rusted_heavymace, magistuarmory:club, magistuarmory:messer_sword

### addWearableArmorDecorationItem (1)

magistuarmory:minicrown_decoration

### items
      .register (3)

magistuarmory:barding, magistuarmory:chainmail_horse_armor, magistuarmory:medieval_bag

### items.register (3)

magistuarmory:tin_ingot, magistuarmory:silver_ingot, magistuarmory:bronze_ingot

## Dynamic families

Weapon families: STILETTOS, SHORT_SWORDS, KATZBALGERS, PIKES, RANSEURS, AHLSPIESSES, GIANT_LANCES, BASTARD_SWORDS, ESTOCS, CLAYMORS, ZWEIHANDERS, FLAME_BLADED_SWORDS, LOCHABER_AXES, CONCAVE_EDGED_HALBERDS, HEAVY_MACES, HEAVY_WAR_HAMMERS, LUCERNE_HAMMERS, MORNINGSTARS, FLAILS, GUISARMES

Shield families: HEATER_SHIELDS, TARGETS, BUCKLERS, RONDACHES, TARTSCHES, ELLIPTICAL_SHIELDS, ROUND_SHIELDS, PAVISES, KITE_SHIELDS

## Exact code leads

- EpicKnights.init(): components, effects, packets, blocks, block entities, events, armor types, items, recipes, creative tabs and reload listeners.
- ModItemsProvider.init(): deferred registration, armor and weapon configuration gates via isDisabled().
- ItemsSupply(): 11 tier instantiations including Netherite fire-resistant item properties.
- ModPackets.init(): Architectury C2S PacketLanceCollision receiver.
- MedievalShieldItem(): BLOCKS_ATTACKS component, shield damage/block sound, durability, repair tags; weight >=10 movement penalty on inventory tick.
- GeneralConfig(): default mob equipment and hard-mode selection; toggles for reach, lance, armor piercing, laceration, two-handed, weapon blocking.
- Recipe classes: ArmorDecorationRecipe, DecorationRemoveRecipe, HeraldryRecipe; custom behavior cannot be reduced to vanilla shaped recipe.
- Pavise: 11 main blocks plus upper-collision helper; analyze placement/removal and block entity persistence.

## ABI follow-through and verification

- The existing moddedmc:items v1 add_item copies vanilla carrier components. It does NOT establish arbitrary numeric equipment attributes or original item-component parity; inspect newest ABI branch before raising a generic host-interface request.
- Shield damage reduction, item tick weight debuffs, lance custom packet validation, armor customization, pavise lifecycle and monster equipment require Java method-level mapping and dedicated bot scenarios.
- Asset item definitions (713) do not imply 713 registered items. The 145 literal declarations and the 11-tier dynamic families must be reconciled with original runtime registration and configuration.
- No tests or server scenarios have been run for Epic Knights yet; M0 is a source-audited slice, M1–M3 unimplemented.
