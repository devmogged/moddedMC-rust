# The approved 1.20.1 Ivy house template uses old NBT for vanilla decorations.
# Use Latin-1 as a lossless byte view, just like the palette migration. Match
# complete tags from the pinned release, and fail if the expected layout changes.
function Convert-IvyHouseDecorations([string] $nbt) {
    function NbtName([string] $name) {
        return [string][char]($name.Length -shr 8) + [char]($name.Length -band 255) + $name
    }
    function NbtHeader([int] $type, [string] $name) {
        return [string][char]$type + (NbtName $name)
    }
    function NbtString([string] $name, [string] $value) {
        return (NbtHeader 8 $name) + (NbtName $value)
    }
    function NbtInt([int] $value) {
        return [string][char](($value -shr 24) -band 255) +
            [char](($value -shr 16) -band 255) + [char](($value -shr 8) -band 255) + [char]($value -band 255)
    }
    function ReplaceExact([string] $source, [string] $old, [string] $new, [int] $count) {
        if ([regex]::Matches($source, [regex]::Escape($old)).Count -ne $count) {
            throw "Unexpected Ivy decoration tag count (expected $count)"
        }
        return $source.Replace($old, $new)
    }
    $end = [string][char]0
    $oneCompound = [string][char]10 + (NbtInt 1)

    # BannerBlockEntity reads modern lower-case pattern/color string pairs.
    $nbt = ReplaceExact $nbt (NbtHeader 9 'Patterns') (NbtHeader 9 'patterns') 7
    $patterns = @{
        ms = 'stripe_middle'; mr = 'rhombus'; flo = 'flower'; tt = 'triangle_top'
        cbo = 'curly_border'; mc = 'circle'; hh = 'half_horizontal'; sku = 'skull'
        sc = 'straight_cross'; bri = 'bricks'; bts = 'triangles_bottom'
    }
    $colors = @('white','orange','magenta','light_blue','yellow','lime','pink','gray',
        'light_gray','cyan','purple','blue','brown','green','red','black')
    $pattern = [regex]::Escape((NbtHeader 8 'Pattern')) + '(?<code>\x00[\x02\x03][a-z]{2,3})' +
        [regex]::Escape((NbtHeader 3 'Color')) + '\x00\x00\x00(?<color>[\x00-\x0f])' + $end
    if ([regex]::Matches($nbt, $pattern).Count -ne 29) { throw 'Unexpected Ivy banner layers' }
    $nbt = [regex]::Replace($nbt, $pattern, {
        param($match)
        $code = $match.Groups['code'].Value.Substring(2)
        $color = [int][char]$match.Groups['color'].Value[0]
        if (!$patterns.ContainsKey($code)) { throw "Unknown Ivy banner pattern $code" }
        (NbtString 'pattern' ('minecraft:' + $patterns[$code])) + (NbtString 'color' $colors[$color]) + $end
    })

    # Seven skull block entities and the framed Armaralis Egg use SkullOwner.
    # The modern profile is an id plus a list of named texture properties.
    $profile = [regex]::Escape((NbtHeader 10 'SkullOwner') + (NbtHeader 11 'Id') + (NbtInt 4)) +
        '(?<uuid>.{16})' + [regex]::Escape((NbtHeader 10 'Properties') + (NbtHeader 9 'textures') +
        $oneCompound + (NbtHeader 8 'Value')) + '(?<value>..[A-Za-z0-9+/=]+)' + ($end * 3)
    $profile = '(?s)' + $profile
    if ([regex]::Matches($nbt, $profile).Count -ne 8) { throw 'Unexpected Ivy skull profiles' }
    $nbt = [regex]::Replace($nbt, $profile, {
        param($match)
        (NbtHeader 10 'profile') + (NbtHeader 11 'id') + (NbtInt 4) + $match.Groups['uuid'].Value +
            (NbtHeader 9 'properties') + $oneCompound + (NbtString 'name' 'textures') +
            (NbtHeader 8 'value') + $match.Groups['value'].Value + ($end * 2)
    })

    # The 30 books have count 1, but update the old byte Count explicitly.
    $book = (NbtString 'id' 'minecraft:book')
    $oldCount = (NbtHeader 1 'Count') + [char]1
    $newCount = (NbtHeader 3 'count') + (NbtInt 1)
    $nbt = ReplaceExact $nbt ($book + $oldCount) ($book + $newCount) 30

    # The invisible frame holds a vanilla player head, not a registered egg.
    # Preserve its profile, name, and -20% mainhand movement speed modifier.
    $head = NbtString 'id' 'minecraft:player_head'
    $old = $head + $oldCount + (NbtHeader 10 'tag') + (NbtHeader 10 'BlockEntityTag') +
        (NbtString 'id' 'minecraft:skull') + $end + (NbtHeader 10 'profile')
    $new = $head + $newCount + (NbtHeader 10 'components') + (NbtHeader 10 'minecraft:block_entity_data') +
        (NbtString 'id' 'minecraft:skull') + $end + (NbtHeader 10 'minecraft:profile')
    $nbt = ReplaceExact $nbt $old $new 1
    # shortcut: Pumpkin CustomNameImpl reads plain strings only; use a styled
    # text component when the host accepts one without rejecting the item.
    $oldName = '{"italic":false,"extra":[{"text":""},{"color":"yellow","text":"Armaralis Egg"}],"text":""}'
    $old = (NbtHeader 10 'display') + (NbtString 'Name' $oldName) + $end
    $nbt = ReplaceExact $nbt $old (NbtString 'minecraft:custom_name' 'Armaralis Egg') 1
    $oldModifier = [regex]::Escape((NbtHeader 9 'AttributeModifiers') + $oneCompound + (NbtHeader 6 'Amount')) +
        '(?<amount>.{8})' + [regex]::Escape((NbtString 'Slot' 'mainhand') +
        (NbtString 'AttributeName' 'minecraft:generic.movement_speed') + (NbtHeader 3 'Operation') +
        (NbtInt 1) + (NbtHeader 11 'UUID') + (NbtInt 4)) + '(?<uuid>.{16})' + $end
    $oldModifier = '(?s)' + $oldModifier
    if ([regex]::Matches($nbt, $oldModifier).Count -ne 1) { throw 'Unexpected Ivy frame modifier' }
    $nbt = [regex]::Replace($nbt, $oldModifier, {
        param($match)
        $hex = -join ($match.Groups['uuid'].Value.ToCharArray() | ForEach-Object { '{0:x2}' -f [int]$_ })
        $uuid = $hex.Insert(20,'-').Insert(16,'-').Insert(12,'-').Insert(8,'-')
        (NbtHeader 9 'minecraft:attribute_modifiers') + $oneCompound +
            (NbtString 'type' 'minecraft:movement_speed') + (NbtString 'id' ('minecraft:' + $uuid)) +
            (NbtHeader 6 'amount') + $match.Groups['amount'].Value +
            (NbtString 'operation' 'add_multiplied_base') + (NbtString 'slot' 'mainhand') + $end
    })
    return $nbt
}
