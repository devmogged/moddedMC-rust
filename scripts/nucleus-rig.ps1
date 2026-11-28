# Convert the released Java keyframe definitions to a local companion rig.
# Generated model/animation files live in the staged pack, never in git.
function Add-NucleusRig($files, [string] $assets, [string] $sourceFile) {
    if (!(Test-Path -LiteralPath $sourceFile)) {
        Write-Warning 'Nucleus animation source is absent; the block display remains static.'
        return
    }
    $animations = [ordered]@{}
    $clip = $null
    $channel = $null
    foreach ($line in [IO.File]::ReadLines($sourceFile)) {
        if ($line -match 'AnimationDefinition (IDLE|SPAWN) = AnimationDefinition.Builder.withLength\(([\d.]+)F\)') {
            $clip = $Matches[1].ToLowerInvariant()
            $animations[$clip] = [ordered]@{ loop = $true; animation_length = [double]$Matches[2]; bones = [ordered]@{} }
        } elseif ($line -match 'addAnimation\("([^"]+)", new AnimationChannel\(AnimationChannel.Targets.(ROTATION|POSITION|SCALE)') {
            $bone = $Matches[1]
            $channel = $Matches[2].ToLowerInvariant()
            if (!$animations[$clip].bones.Contains($bone)) { $animations[$clip].bones[$bone] = [ordered]@{} }
            $animations[$clip].bones[$bone][$channel] = [ordered]@{}
        } elseif ($line -match 'new Keyframe\(([\d.]+)F, KeyframeAnimations\.\w+Vec\((-?[\d.]+)F, (-?[\d.]+)F, (-?[\d.]+)F\), AnimationChannel.Interpolations.(LINEAR|CATMULLROM)') {
            $time = $Matches[1]
            $vector = @([double]$Matches[2], [double]$Matches[3], [double]$Matches[4])
            $frame = [ordered]@{ vector = $vector }
            if ($Matches[5] -eq 'CATMULLROM') { $frame['easing'] = 'catmullrom' }
            $animations[$clip].bones[$bone][$channel][$time] = $frame
        }
    }
    if ($animations.Count -ne 2 -or $animations.spawn.bones.group.rotation.Count -ne 481 -or
        $animations.spawn.bones.group.scale.Count -ne 481 -or $animations.idle.bones.root.position.Count -ne 5) {
        throw 'Unexpected released nucleus animation data.'
    }
    $bones = @(
        @{ name = 'root'; pivot = @(0, 0, 0) },
        @{ name = 'group'; parent = 'root'; pivot = @(0, 0, 0) },
        @{ name = 'outerlayer'; parent = 'group'; pivot = @(0, 0, 0); cubes = @(@{ origin = @(-6,-6,-6); size = @(12,12,12); uv = @(0,0) }) },
        @{ name = 'innerlayer'; parent = 'outerlayer'; pivot = @(0, 0, 0); cubes = @(@{ origin = @(-4,-4,-4); size = @(8,8,8); uv = @(0,24) }) }
    )
    $geometry = @{ format_version = '1.12.0'; 'minecraft:geometry' = @(@{
        description = @{ identifier = 'geometry.draconian_nucleus'; texture_width = 48; texture_height = 48; visible_bounds_width = 3; visible_bounds_height = 3; visible_bounds_offset = @(0,0,0) }
        bones = $bones
    }) }
    $files["$assets/geckolib/models/block/draconian_nucleus.geo.json"] = $geometry | ConvertTo-Json -Depth 16 -Compress
    $files["$assets/geckolib/animations/block/draconian_nucleus.animation.json"] = @{ format_version = '1.8.0'; animations = $animations } | ConvertTo-Json -Depth 16 -Compress
    for ($index = 0; $index -lt 2; $index++) {
        $files["$assets/moddedmc/rigs/moddedmc/display/draconian_nucleus/$index.json"] = @{
            geometry = 'saintsdragons:block/draconian_nucleus'; animations = 'saintsdragons:block/draconian_nucleus'
            texture = 'saintsdragons:textures/block/draconic_nucleus.png'; clip = 'idle'; transition_ticks = 0; author = 'Leon Saint'
        } | ConvertTo-Json -Compress
    }
}
