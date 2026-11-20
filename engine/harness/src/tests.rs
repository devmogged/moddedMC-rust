#![allow(clippy::unwrap_used)]

#[test]
fn scenario_keeps_source_lines_and_requires_allow_reasons() {
    let source = "seed 12\narea -1 70 0 1 72 0 # inclusive\nstart\ntick 3\nsnapshot ready\nstop\n";
    let scenario = crate::scenario::parse(source).unwrap();
    assert_eq!(scenario.steps[1].line, 4);
    assert_eq!(scenario.area.min, [-1, 70, 0]);
    assert!(crate::scenario::parse(&source.replace("tick 3", "allow block")).is_err());
    assert!(crate::scenario::parse(&source.replace("tick 3", "cmd /tick unfreeze")).is_err());
    assert!(
        crate::scenario::parse(&source.replace("tick 3", "probe execute if entity @s run kill @s"))
            .is_err()
    );
    assert!(
        crate::scenario::parse(&source.replace(
            "tick 3",
            "probe execute positioned 4.5 70 3.5 if entity @e[distance=..0.1]",
        ))
        .is_ok()
    );
    assert!(
        crate::scenario::parse(&source.replace(
            "tick 3",
            "probe execute positioned 4.5 70 3.5 if entity @s run kill @s",
        ))
        .is_err()
    );
}
