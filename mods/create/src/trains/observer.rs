//! Source-derived TrackObserver activation and persisted fields (Create 6.0.10).
//! Edge-point registration, train filtering and redstone activation are not wired yet.

use fastnbt::{IntArray, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub(super) struct TrackObserver {
    activated: i32,
    current_train: Option<[i32; 4]>,
    filter: HashMap<String, Value>,
}

impl TrackObserver {
    pub fn is_activated(&self) -> bool {
        self.activated > 0
    }

    // TrackObserver.keepAlive resets the pulse to eight graph ticks.
    pub fn keep_alive(&mut self, train: [i32; 4]) {
        self.activated = 8;
        self.current_train = Some(train);
    }

    // Java decrements before clearing the train on the final tick.
    pub fn tick(&mut self) {
        if self.is_activated() {
            self.activated -= 1;
        }
        if !self.is_activated() {
            self.current_train = None;
        }
    }

    /// Read only the observer-specific fields, leaving base TrackEdgePoint metadata to its codec.
    pub fn from_fields(nbt: &HashMap<String, Value>) -> Result<Self, String> {
        let activated = match nbt.get("Activated") {
            Some(Value::Int(v)) => *v,
            None => 0,
            _ => return Err("invalid TrackObserver Activated".into()),
        };
        let filter = match nbt.get("Filter") {
            Some(Value::Compound(v)) => v.clone(),
            None => HashMap::new(),
            _ => return Err("invalid TrackObserver Filter".into()),
        };
        let current_train = match nbt.get("TrainId") {
            Some(Value::IntArray(v)) => Some(
                <[i32; 4]>::try_from(v.as_ref())
                    .map_err(|_| "invalid TrackObserver TrainId UUID length")?,
            ),
            None => None,
            _ => return Err("invalid TrackObserver TrainId".into()),
        };
        Ok(Self {
            activated,
            current_train,
            filter,
        })
    }

    /// Update observer fields without deleting inherited edge-point NBT.
    pub fn write_fields(&self, nbt: &mut HashMap<String, Value>) {
        nbt.insert("Activated".into(), Value::Int(self.activated));
        nbt.insert("Filter".into(), Value::Compound(self.filter.clone()));
        if let Some(id) = self.current_train {
            nbt.insert(
                "TrainId".into(),
                Value::IntArray(IntArray::new(id.to_vec())),
            );
        } else {
            nbt.remove("TrainId");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRAIN_A: [i32; 4] = [1, 2, -3, 4];
    const TRAIN_B: [i32; 4] = [5, 6, 7, 8];

    #[test]
    fn keep_alive_pulses_for_exactly_eight_graph_ticks() {
        let mut point = TrackObserver::default();
        point.keep_alive(TRAIN_A);
        for _ in 0..7 {
            assert!(point.is_activated());
            assert_eq!(point.current_train, Some(TRAIN_A));
            point.tick();
        }
        assert!(point.is_activated());
        point.tick();
        assert!(!point.is_activated());
        assert_eq!(point.current_train, None);
        point.tick();
        assert_eq!(point.activated, 0);
    }

    #[test]
    fn next_train_refreshes_pulse_and_replaces_current_id() {
        let mut point = TrackObserver::default();
        point.keep_alive(TRAIN_A);
        for _ in 0..5 {
            point.tick();
        }
        point.keep_alive(TRAIN_B);
        assert_eq!(point.activated, 8);
        assert_eq!(point.current_train, Some(TRAIN_B));
    }

    #[test]
    fn observer_fields_roundtrip_preserves_base_point_and_filter() {
        let mut fields = HashMap::from([
            ("Position".into(), Value::Double(7.5)),
            (
                "Filter".into(),
                Value::Compound(HashMap::from([
                    ("id".into(), Value::String("minecraft:coal".into())),
                    ("Count".into(), Value::Int(5)),
                ])),
            ),
        ]);
        let mut point = TrackObserver::from_fields(&fields).unwrap();
        point.keep_alive(TRAIN_A);
        point.write_fields(&mut fields);
        let bytes = fastnbt::to_bytes(&fields).unwrap();
        let decoded: HashMap<String, Value> = fastnbt::from_bytes(&bytes).unwrap();
        let loaded = TrackObserver::from_fields(&decoded).unwrap();
        assert_eq!(loaded.current_train, Some(TRAIN_A));
        assert_eq!(loaded.activated, 8);
        assert!(matches!(decoded.get("Position"), Some(Value::Double(7.5))));
        assert!(
            matches!(loaded.filter.get("id"), Some(Value::String(id)) if id == "minecraft:coal")
        );
        assert!(matches!(loaded.filter.get("Count"), Some(Value::Int(5))));
        point.tick();
    }

    #[test]
    fn absent_optional_fields_default_to_inactive_and_empty_filter() {
        let point = TrackObserver::from_fields(&HashMap::new()).unwrap();
        assert!(!point.is_activated());
        assert!(point.filter.is_empty());
        assert_eq!(point.current_train, None);
    }

    #[test]
    fn malformed_uuid_or_filter_is_rejected_without_loss() {
        let mut fields = HashMap::from([(
            "TrainId".into(),
            Value::IntArray(IntArray::new(vec![1, 2, 3])),
        )]);
        assert!(
            TrackObserver::from_fields(&fields)
                .unwrap_err()
                .contains("UUID")
        );
        fields.insert(
            "TrainId".into(),
            Value::IntArray(IntArray::new(vec![1, 2, 3, 4])),
        );
        fields.insert("Filter".into(), Value::String("invalid".into()));
        assert!(
            TrackObserver::from_fields(&fields)
                .unwrap_err()
                .contains("Filter")
        );
    }

    #[test]
    fn write_without_train_removes_stale_train_id() {
        let point = TrackObserver::default();
        let mut fields = HashMap::from([(
            "TrainId".into(),
            Value::IntArray(IntArray::new(TRAIN_A.to_vec())),
        )]);
        point.write_fields(&mut fields);
        assert!(!fields.contains_key("TrainId"));
        assert!(fields.contains_key("Filter"));
    }
}
