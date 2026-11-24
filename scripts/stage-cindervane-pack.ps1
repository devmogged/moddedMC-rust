# Build a local 26.3 companion resource pack from the approved Saint's Dragons release.
param(
    [Parameter(Mandatory)] [string] $Jar,
    [string] $OutFile,
    [switch] $AllDragons
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$source = (Resolve-Path -LiteralPath $Jar).Path
$releaseHash = '1093be9e314248646162bf8362e30cee63f6c6ed57d1f6590ce784cc69aee532'
if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $releaseHash) {
    throw 'Expected the official Saint''s Dragons 0.9.88 Fabric JAR for Minecraft 1.20.1.'
}
if (-not $OutFile) {
    $name = if ($AllDragons) { 'saints-dragons' } else { 'cindervane' }
    $OutFile = Join-Path (Split-Path $PSScriptRoot -Parent) "target/client-packs/$name-0.9.88.zip"
}
$destination = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutFile)
if ([IO.Path]::GetExtension($destination) -ne '.zip') { throw 'OutFile must end in .zip.' }
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
$scratch = "$destination.$([Guid]::NewGuid()).tmp"
$assets = 'assets/saintsdragons'
$copies = [ordered]@{}
$files = [ordered]@{
    'pack.mcmeta' = '{"pack":{"description":"Saint''s Dragons by Leon Saint","min_format":[97,1],"max_format":[97,1]}}'
}
$species = @('cindervane')
if ($AllDragons) {
    $species += @('atroxiia','ignivorus','moop','mossback','nulljaw','raevyx','stegonaut','varasuchus','volitans')
}
foreach ($name in $species) {
    $entity = "entity/$name/$name"
    $copies["$assets/geo/$entity.geo.json"] = "$assets/geckolib/models/$entity.geo.json"
    $copies["$assets/animations/$entity.animation.json"] = "$assets/geckolib/animations/$entity.animation.json"
    $copies["$assets/textures/$entity.png"] = "$assets/textures/$entity.png"
    $rig = [ordered]@{
        geometry = "saintsdragons:$entity"
        animations = "saintsdragons:$entity"
        texture = "saintsdragons:textures/$entity.png"
        clip = "animation.$name.idle"
        yaw_offset = 180
        author = 'Leon Saint'
    }
    if ($name -eq 'cindervane') {
        # The stock body display is 1.85x and sits 1.15 blocks above the feet.
        $rig.scale = 1.0 / 1.85
        $rig.y_offset = -1.15 / 1.85
        $rig.hide_parts = @('head','tail','wing_left','wing_right') | ForEach-Object { "saintsdragons:cindervane_$_" }
    }
    $variantStems = switch ($name) {
        'cindervane' { @{ albino = 'cindervane_albino'; piebald = 'cindervane_piebald' } }
        'ignivorus' { @{ crimson = 'crimson_ignivorus' } }
        'raevyx' { @{ night_gold = 'raevyx_night_gold' } }
        'varasuchus' { @{ void_kissed = 'varasuchus_void_kissed' } }
        'volitans' { @{ bloodshot = 'volitans_bloodshot' } }
        default { @{} }
    }
    if ($name -notin @('moop', 'mossback')) {
        $rig.textures = [ordered]@{}
        $variantStems['default'] = $name
        foreach ($variant in ($variantStems.Keys | Sort-Object)) {
            $stem = $variantStems[$variant]
            $femalePath = "textures/entity/$name/${stem}_female.png"
            $copies["$assets/$femalePath"] = "$assets/$femalePath"
            $rig.textures["${variant}_female"] = "saintsdragons:$femalePath"
            if ($variant -ne 'default') {
                $path = "textures/entity/$name/$stem.png"
                $copies["$assets/$path"] = "$assets/$path"
                $rig.textures[$variant] = "saintsdragons:$path"
            }
        }
    }
    $files["$assets/moddedmc/rigs/${name}_display.json"] = $rig | ConvertTo-Json -Compress
}
if ($AllDragons) {
    foreach ($name in @('latcher','winged','whettled','ivy_oleander')) {
        $entity = if ($name -eq 'ivy_oleander') { "entity/$name/$name" } else { "entity/draconian_swarm/$name/$name" }
        $copies["$assets/geo/$entity.geo.json"] = "$assets/geckolib/models/$entity.geo.json"
        $copies["$assets/animations/$entity.animation.json"] = "$assets/geckolib/animations/$entity.animation.json"
        $copies["$assets/textures/$entity.png"] = "$assets/textures/$entity.png"
        $idle = if ($name -eq 'whettled') { 'idleandslowmove' } else { 'idle' }
        $files["$assets/moddedmc/rigs/${name}_display.json"] = [ordered]@{
            geometry = "saintsdragons:$entity"
            animations = "saintsdragons:$entity"
            texture = "saintsdragons:textures/$entity.png"
            clip = "$name.animation.$idle"
            yaw_offset = 180
            author = 'Leon Saint'
        } | ConvertTo-Json -Compress
    }
    foreach ($name in @('dragonheart_alloy_block','dragonheart_block','deepslate_worldroot_ore','dragonheart_ore','worldroot_block','raw_worldroot_block','draconic_crucible','draconian_pellucida')) {
        $model = "$assets/models/block/$name.json"
        $texture = "$assets/textures/block/$name.png"
        $copies[$model] = $model
        $copies[$texture] = $texture
    }
    foreach ($relative in @(
        'models/block/draconian_nucleus.json',
        'models/item/draconian_nucleus.json',
        'models/item/draconian_controller.json',
        'models/item/ivy_octopus_plushie.json',
        'textures/item/ivy_octopus_plushie.png',
        'models/item/draconic_codex.json',
        'textures/item/draconic_codex.png',
        'textures/block/draconic_nucleus.png',
        'textures/item/draconian_swarm/draconian_controller.png'
    )) {
        $copies["$assets/$relative"] = "$assets/$relative"
    }
    $crucibleItem = "$assets/models/item/draconic_crucible.json"
    $copies[$crucibleItem] = $crucibleItem
    $incubatorAssets = @(
        'models/block/ignivorus_incubator_block.json',
        'textures/block/ignivorus_incubator.png',
        'textures/block/ignivorus_incubator.png.mcmeta',
        'textures/block/ignivorus_incubator_top.png',
        'textures/block/ignivorus_incubator_top.png.mcmeta'
    )
    foreach ($relative in $incubatorAssets) {
        $path = "$assets/$relative"
        $copies[$path] = $path
    }
    foreach ($path in @(
        'models/block/ignivorus/ignivorus_egg_stage0.json',
        'models/block/ignivorus/ignivorus_egg_stage1.json',
        'models/block/ignivorus/ignivorus_egg_stage2.json',
        'models/item/ignivorus_egg.json',
        'textures/block/ignivorus/ignivorus_egg.png',
        'textures/block/ignivorus/ignivorus_egg_slightly_cracked.png',
        'textures/block/ignivorus/ignivorus_egg_very_cracked.png',
        'textures/item/ignivorus/ignivorus_egg_item.png',
        'models/block/stegonaut/stegonaut_egg_stage0.json',
        'models/block/stegonaut/stegonaut_egg_stage1.json',
        'models/block/stegonaut/stegonaut_egg_stage2.json',
        'models/item/stegonaut_egg.json',
        'textures/block/stegonaut/stegonaut_egg.png',
        'textures/block/stegonaut/stegonaut_egg_slightly_cracked.png',
        'textures/block/stegonaut/stegonaut_egg_very_cracked.png',
        'textures/item/stegonaut/stegonaut_egg_item.png',
        'models/block/varasuchus/varasuchus_egg_stage0.json',
        'models/block/varasuchus/varasuchus_egg_stage1.json',
        'models/block/varasuchus/varasuchus_egg_stage2.json',
        'models/item/varasuchus_egg.json',
        'textures/block/varasuchus/varasuchus_egg.png',
        'textures/block/varasuchus/varasuchus_egg_slightly_cracked.png',
        'textures/block/varasuchus/varasuchus_egg_very_cracked.png',
        'textures/item/varasuchus/varasuchus_egg_item.png',
        'models/block/atroxiia/atroxiia_egg_stage0.json',
        'models/block/atroxiia/atroxiia_egg_stage1.json',
        'models/block/atroxiia/atroxiia_egg_stage2.json',
        'models/item/atroxiia_egg.json',
        'textures/block/atroxiia/atroxiia_egg.png',
        'textures/block/atroxiia/atroxiia_egg_slightly_cracked.png',
        'textures/block/atroxiia/atroxiia_egg_very_cracked.png',
        'models/block/cindervane/cindervane_egg_1_stage0.json',
        'models/block/cindervane/cindervane_egg_1_stage1.json',
        'models/block/cindervane/cindervane_egg_1_stage2.json',
        'models/block/cindervane/cindervane_egg_2_stage0.json',
        'models/block/cindervane/cindervane_egg_2_stage1.json',
        'models/block/cindervane/cindervane_egg_2_stage2.json',
        'models/block/cindervane/cindervane_egg_3_stage0.json',
        'models/block/cindervane/cindervane_egg_3_stage1.json',
        'models/block/cindervane/cindervane_egg_3_stage2.json',
        'models/item/cindervane_egg.json',
        'textures/block/cindervane/cindervane_egg.png',
        'textures/block/cindervane/cindervane_egg_slightly_cracked.png',
        'textures/block/cindervane/cindervane_egg_very_cracked.png',
        'textures/item/cindervane/cindervane_egg_item.png',
        'models/block/raevyx/raevyx_egg_stage0.json',
        'models/block/raevyx/raevyx_egg_stage1.json',
        'models/block/raevyx/raevyx_egg_stage2.json',
        'models/item/raevyx_egg.json',
        'textures/block/raevyx/raevyx_egg.png',
        'textures/block/raevyx/raevyx_egg_slightly_cracked.png',
        'textures/block/raevyx/raevyx_egg_very_cracked.png',
        'textures/item/raevyx/raevyx_egg_item.png',
        'models/block/volitans/volitans_egg_stage0.json',
        'models/block/volitans/volitans_egg_stage1.json',
        'models/block/volitans/volitans_egg_stage2.json',
        'models/item/volitans_egg.json',
        'textures/block/volitans/volitans_egg.png',
        'textures/block/volitans/volitans_egg_slightly_cracked.png',
        'textures/block/volitans/volitans_egg_very_cracked.png',
        'textures/item/volitans/volitans_egg_item.png',
        'textures/item/atroxiia/atroxiia_egg.png'
    )) {
        $copies["$assets/$path"] = "$assets/$path"
    }
    foreach ($name in @('ignivorus','stegonaut','varasuchus','atroxiia','volitans','cindervane','raevyx')) {
        $baby = "entity/$name/baby_$name"
        $copies["$assets/geo/$baby.geo.json"] = "$assets/geckolib/models/$baby.geo.json"
        $copies["$assets/animations/$baby.animation.json"] = "$assets/geckolib/animations/$baby.animation.json"
        $copies["$assets/textures/$baby.png"] = "$assets/textures/$baby.png"
        $copies["$assets/textures/${baby}_female.png"] = "$assets/textures/${baby}_female.png"
        $files["$assets/moddedmc/rigs/baby_${name}_display.json"] = [ordered]@{
            geometry = "saintsdragons:$baby"
            animations = "saintsdragons:$baby"
            texture = "saintsdragons:textures/$baby.png"
            textures = @{ default_female = "saintsdragons:textures/${baby}_female.png" }
            clip = "animation.$name.idle"
            yaw_offset = 180
            author = 'Leon Saint'
        } | ConvertTo-Json -Compress
    }
    $dropTextures = [ordered]@{
        hearty_dragon_meal = 'hearty_dragon_meal'
        creative_dragon_gender_wand = 'creative_dragon_gender_wand'
        creative_dragon_variant_brush = 'creative_dragon_variant_brush'
        creative_dragon_tame_meal = 'creative_dragon_tame_meal'
        dragon_brush = 'grooming_brush'
        scale_plucker = 'scale_plucker'
        golden_dragon_brush = 'golden_grooming_brush'
        ignivorus_heart = 'ignivorus/ignivorus_heart'
        ignivorus_tooth = 'ignivorus/ignivorus_tooth'
        ignivorus_wing_hide = 'ignivorus_wing_hide'
        raevyx_wing_hide = 'raevyx_wing_hide'
        raevyx_wingtalon = 'raevyx_wingtalon'
        raw_mossback = 'mossback/raw_mossback'
        cooked_mossback = 'mossback/cooked_mossback'
        bucket_of_moop = 'bucket_of_moop'
        raw_moop = 'moop_item_raw'
        cooked_moop = 'moop_item_cooked'
        potion_of_tideguard = 'potion_of_tideguard'
        potion_of_searing = 'potion_of_searing'
        raevyx_scale = 'raevyx/raevyx_scale'
        cindervane_scale = 'cindervane/cindervane_scale'
        stegonaut_scale = 'stegonaut/stegonaut_scale'
        varasuchus_scale = 'varasuchus/varasuchus_scale'
        ignivorus_scale = 'ignivorus/ignivorus_scale'
        volitans_scale = 'volitans/volitans_scale'
        volitans_spine = 'volitans/volitans_spine'
        arrow_of_venom = 'arrow_of_venom'
        atroxiia_scale = 'atroxiia/atroxiia_scale'
        draconian_flesh = 'draconian_swarm/draconian_flesh'
        searing_coal = 'searing_coal'
        ancient_dragonite_fragment = 'ancient_dragonite_fragment'
        dragon_seal_stone = 'dragon_seal_stone'
        dragon_binder_core = 'dragon_binder_core'
        raevyx_binder = 'raevyx/raevyx_binder'
        cindervane_binder = 'cindervane/cindervane_binder'
        stegonaut_binder = 'stegonaut/stegonaut_binder'
        varasuchus_binder = 'varasuchus/varasuchus_binder'
        ignivorus_binder = 'ignivorus/ignivorus_binder'
        volitans_binder = 'volitans/volitans_binder'
        nulljaw_binder = 'nulljaw/nulljaw_binder'
        atroxiia_binder = 'atroxiia/atroxiia_binder'
        raw_worldroot = 'raw_worldroot'
        worldroot_ingot = 'worldroot_ingot'
        draconian_helmet = 'armors/draconian_helmet'
        draconian_chestplate = 'armors/draconian_chestplate'
        draconian_leggings = 'armors/draconian_leggings'
        draconian_boots = 'armors/draconian_boots'
        blood_tempest_helmet = 'armors/blood_tempest_helmet'
        blood_tempest_chestplate = 'armors/blood_tempest_chestplate'
        blood_tempest_leggings = 'armors/blood_tempest_leggings'
        blood_tempest_boots = 'armors/blood_tempest_boots'
        dragonlord_helmet = 'armors/dragonlord_helmet'
        dragonlord_chestplate = 'armors/dragonlord_chestplate'
        dragonlord_leggings = 'armors/dragonlord_leggings'
        dragonlord_boots = 'armors/dragonlord_boots'
        blood_tempest_katana = 'tools/blood_tempest_katana'
        dragonlord_sword = 'tools/dragonlord_sword'
        worldroot_sword = 'tools/worldroot_sword'
        worldroot_pickaxe = 'tools/worldroot_pickaxe'
        worldroot_axe = 'tools/worldroot_axe'
        worldroot_shovel = 'tools/worldroot_shovel'
        worldroot_hoe = 'tools/worldroot_hoe'
        bleeding_bolt_music_disc = 'bleeding_bolt_music_disc'
        mossback_music_music_disc = 'mossback_music_music_disc'
        dragonheart_chunk = 'dragonheart_chunk'
        dragonheart_alloy = 'dragonheart_alloy'
    }
    foreach ($name in $dropTextures.Keys) {
        $model = "$assets/models/item/$name.json"
        $texture = "$assets/textures/item/$($dropTextures[$name]).png"
        $copies[$model] = $model
        $copies[$texture] = $texture
    }
}

try {
    $inputJar = [IO.Compression.ZipFile]::OpenRead($source)
    try {
        $pack = [IO.Compression.ZipFile]::Open($scratch, [IO.Compression.ZipArchiveMode]::Create)
        try {
            if ($AllDragons) {
                $copies['data/saintsdragons/structures/draconian_nucleus.nbt'] = 'data/saintsdragons/structure/draconian_nucleus.nbt'
                foreach ($entry in $inputJar.Entries) {
                    $path = $entry.FullName
                    if ($path -eq "$assets/sounds.json" -or
                        $path -cmatch '^assets/saintsdragons/sounds/[a-z0-9_./-]+\.ogg$') {
                        $copies[$path] = $path
                    }
                }
            }
            foreach ($path in $copies.Keys) {
                $entry = $inputJar.GetEntry($path)
                if (-not $entry) { throw "Missing release asset $path" }
                $reader = $entry.Open()
                try {
                    $writer = $pack.CreateEntry($copies[$path]).Open()
                    try {
                        if ($entry.Length -gt 8MB -and $path.EndsWith('.animation.json')) {
                            # Remove JSON whitespace outside strings while preserving every token.
                            $text = [IO.StreamReader]::new($reader)
                            try { $json = $text.ReadToEnd() } finally { $text.Dispose() }
                            $compact = [regex]::Replace($json, '"(?:\\.|[^"\\])*"|[ \t\r\n]+', {
                                param($match)
                                if ($match.Value[0] -eq '"') { $match.Value } else { '' }
                            })
                            $bytes = [Text.Encoding]::UTF8.GetBytes($compact)
                            if ($bytes.Length -gt 8MB) { throw "Animation exceeds the host's asset limit: $path" }
                            $writer.Write($bytes, 0, $bytes.Length)
                        } else {
                            $reader.CopyTo($writer)
                        }
                    } finally { $writer.Dispose() }
                } finally { $reader.Dispose() }
            }
            foreach ($path in $files.Keys) {
                $writer = [IO.StreamWriter]::new($pack.CreateEntry($path).Open(), [Text.UTF8Encoding]::new($false))
                try { $writer.Write($files[$path]) } finally { $writer.Dispose() }
            }
        } finally { $pack.Dispose() }
    } finally { $inputJar.Dispose() }
    if ([IO.File]::Exists($destination)) {
        [IO.File]::Replace($scratch, $destination, [NullString]::Value)
    } else {
        [IO.File]::Move($scratch, $destination)
    }
} finally {
    if ([IO.File]::Exists($scratch)) { [IO.File]::Delete($scratch) }
}
Write-Output $destination
