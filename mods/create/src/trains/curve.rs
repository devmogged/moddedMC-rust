//! NBT for Create 6.0.10 BezierConnection.write/read(BlockPos.ZERO).
//! Catnip VecHelper encodes each vector as a compound with a V list of doubles.

use super::{bezier::BezierGeometry, math::Vec3};
use fastnbt::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A directed curve. BezierConnection.secondary() swaps endpoints and primary.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CurveRecord {
    pub block_positions: [[i32; 3]; 2],
    pub starts: [Vec3; 2],
    pub axes: [Vec3; 2],
    pub normals: [Vec3; 2],
    pub material: String,
    pub primary: bool,
    pub girder: bool,
    pub smoothing: Option<[i32; 2]>,
}

impl CurveRecord {
    pub fn geometry(&self) -> Option<BezierGeometry> {
        let finite = self
            .starts
            .iter()
            .chain(self.axes.iter())
            .chain(self.normals.iter())
            .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite());
        if !finite {
            return None;
        }
        BezierGeometry::new(self.starts, self.axes, self.primary, self.girder)
    }

    pub fn secondary(&self) -> Self {
        Self {
            block_positions: [self.block_positions[1], self.block_positions[0]],
            starts: [self.starts[1], self.starts[0]],
            axes: [self.axes[1], self.axes[0]],
            normals: [self.normals[1], self.normals[0]],
            material: self.material.clone(),
            primary: !self.primary,
            girder: self.girder,
            smoothing: self.smoothing.map(|s| [s[1], s[0]]),
        }
    }

    pub fn to_nbt_fields(&self) -> Result<HashMap<String, Value>, String> {
        if self.geometry().is_none() {
            return Err("invalid Bezier geometry".into());
        }
        let wire = SavedCurve {
            girder: self.girder,
            material: self.material.clone(),
            primary: self.primary,
            positions: self
                .block_positions
                .iter()
                .map(|[x, y, z]| SavedPosition {
                    pos: SavedBlockPos {
                        x: *x,
                        y: *y,
                        z: *z,
                    },
                })
                .collect(),
            starts: self.starts.iter().copied().map(SavedVec::from).collect(),
            axes: self.axes.iter().copied().map(SavedVec::from).collect(),
            normals: self.normals.iter().copied().map(SavedVec::from).collect(),
            smoothing: self
                .smoothing
                .map(|pair| pair.into_iter().map(|value| SavedInt { value }).collect()),
        };
        let data = fastnbt::to_bytes(&wire).map_err(|e| e.to_string())?;
        fastnbt::from_bytes(&data).map_err(|e| e.to_string())
    }

    pub fn from_nbt_fields(fields: &HashMap<String, Value>) -> Result<Self, String> {
        let bytes = fastnbt::to_bytes(fields).map_err(|e| e.to_string())?;
        let wire: SavedCurve = fastnbt::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let block_positions = pair(wire.positions, |p| [p.pos.x, p.pos.y, p.pos.z])?;
        let starts = pair(wire.starts, |p| p.vec())?;
        let axes = pair(wire.axes, |p| p.vec())?;
        let normals = pair(wire.normals, |p| p.vec())?;
        let smoothing = wire
            .smoothing
            .map(|values| pair(values, |int| int.value))
            .transpose()?;
        let curve = Self {
            block_positions,
            starts,
            axes,
            normals,
            // TrackMaterial.deserialize accepts missing/blank ids from old saves.
            material: if wire.material.trim().is_empty() {
                "create:andesite".to_owned()
            } else {
                wire.material
            },
            primary: wire.primary,
            girder: wire.girder,
            smoothing,
        };
        if curve.geometry().is_none() {
            return Err("invalid Bezier geometry".into());
        }
        Ok(curve)
    }
}

fn pair<T, U>(values: Vec<T>, convert: impl Fn(T) -> U) -> Result<[U; 2], String> {
    let [a, b] = <[T; 2]>::try_from(values)
        .map_err(|_| "Create curve requires exactly two entries".to_owned())?;
    Ok([convert(a), convert(b)])
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedCurve {
    #[serde(rename = "Girder")]
    girder: bool,
    #[serde(rename = "Material", default)]
    material: String,
    #[serde(rename = "Primary")]
    primary: bool,
    #[serde(rename = "Positions")]
    positions: Vec<SavedPosition>,
    #[serde(rename = "Starts")]
    starts: Vec<SavedVec>,
    #[serde(rename = "Axes")]
    axes: Vec<SavedVec>,
    #[serde(rename = "Normals")]
    normals: Vec<SavedVec>,
    #[serde(rename = "Smoothing", skip_serializing_if = "Option::is_none", default)]
    smoothing: Option<Vec<SavedInt>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedBlockPos {
    #[serde(rename = "X")]
    x: i32,
    #[serde(rename = "Y")]
    y: i32,
    #[serde(rename = "Z")]
    z: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedPosition {
    #[serde(rename = "Pos")]
    pos: SavedBlockPos,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedVec {
    #[serde(rename = "V")]
    data: Vec<f64>,
}

impl From<Vec3> for SavedVec {
    fn from(vec: Vec3) -> Self {
        Self {
            data: vec![vec.x, vec.y, vec.z],
        }
    }
}

impl SavedVec {
    fn vec(self) -> Vec3 {
        // Wrong vector length causes non-finite geometry, which is rejected.
        let [x, y, z] = <[f64; 3]>::try_from(self.data).unwrap_or([f64::NAN; 3]);
        Vec3 { x, y, z }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedInt {
    #[serde(rename = "V")]
    value: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn fixture() -> CurveRecord {
        CurveRecord {
            block_positions: [[0, 64, 0], [8, 64, 8]],
            starts: [vec3(0.5, 64.0, 0.5), vec3(8.5, 64.0, 8.5)],
            axes: [vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, -1.0)],
            normals: [vec3(0.0, 1.0, 0.0); 2],
            material: "create:brass".to_owned(),
            primary: true,
            girder: true,
            smoothing: Some([2, -3]),
        }
    }

    #[test]
    fn curve_nbt_uses_java_v_compound_and_blockpos_layout() {
        let curve = fixture();
        let fields = curve.to_nbt_fields().unwrap();
        let vec_compounds = match &fields["Starts"] {
            Value::List(values) => values,
            _ => panic!("starts must be list compounds"),
        };
        let first = match &vec_compounds[0] {
            Value::Compound(compound) => compound,
            _ => panic!("starts entry must be compound"),
        };
        assert!(matches!(&first["V"], Value::List(_)));
        assert_eq!(first.len(), 1);
        for key in [
            "Positions",
            "Axes",
            "Normals",
            "Material",
            "Smoothing",
            "Primary",
            "Girder",
        ] {
            assert!(fields.contains_key(key));
        }
        let loaded = CurveRecord::from_nbt_fields(&fields).unwrap();
        assert_eq!(loaded.starts, curve.starts);
        assert_eq!(loaded.axes, curve.axes);
        assert_eq!(loaded.normals, curve.normals);
        assert_eq!(loaded.material, "create:brass");
        assert_eq!(fields["Material"], Value::String("create:brass".into()));
        assert_eq!(loaded.block_positions, curve.block_positions);
        assert_eq!(loaded.smoothing, curve.smoothing);
        assert_eq!(loaded.primary, curve.primary);
        assert_eq!(loaded.girder, curve.girder);
        assert!(loaded.geometry().unwrap().length() > 10.0);
    }

    #[test]
    fn secondary_swaps_positions_vectors_smoothing_and_primary() {
        let curve = fixture();
        let secondary = curve.secondary();
        assert_eq!(
            secondary.block_positions,
            [curve.block_positions[1], curve.block_positions[0]]
        );
        assert_eq!(secondary.starts, [curve.starts[1], curve.starts[0]]);
        assert_eq!(secondary.axes, [curve.axes[1], curve.axes[0]]);
        assert_eq!(secondary.smoothing, Some([-3, 2]));
        assert!(!secondary.primary);
        assert_eq!(secondary.material, curve.material);
        assert!(secondary.girder);
        let roundtrip = CurveRecord::from_nbt_fields(&secondary.to_nbt_fields().unwrap()).unwrap();
        assert_eq!(roundtrip.starts, secondary.starts);
        assert_eq!(secondary.secondary().starts, curve.starts);
    }

    #[test]
    fn optional_smoothing_is_omitted_as_java_does() {
        let mut curve = fixture();
        curve.smoothing = None;
        let fields = curve.to_nbt_fields().unwrap();
        assert!(!fields.contains_key("Smoothing"));
        assert!(
            CurveRecord::from_nbt_fields(&fields)
                .unwrap()
                .smoothing
                .is_none()
        );
    }

    #[test]
    fn legacy_curve_material_defaults_to_java_andesite() {
        // TrackMaterial.deserialize("") in Create migrates old track connections.
        let mut fields = fixture().to_nbt_fields().unwrap();
        fields.remove("Material");
        let loaded = CurveRecord::from_nbt_fields(&fields).unwrap();
        assert_eq!(loaded.material, "create:andesite");
        assert_eq!(
            loaded.to_nbt_fields().unwrap()["Material"],
            Value::String("create:andesite".into())
        );
        fields.insert("Material".into(), Value::String(String::new()));
        assert_eq!(
            CurveRecord::from_nbt_fields(&fields).unwrap().material,
            "create:andesite"
        );
    }

    #[test]
    fn malformed_curves_fail_closed() {
        let fields = fixture().to_nbt_fields().unwrap();
        let mut missing = fields.clone();
        missing.remove("Axes");
        assert!(CurveRecord::from_nbt_fields(&missing).is_err());
        let mut extra = fields.clone();
        extra.insert("Surprise".into(), Value::Int(1));
        assert!(CurveRecord::from_nbt_fields(&extra).is_err());
        let mut curve = fixture();
        curve.starts[0].x = f64::NAN;
        assert!(curve.to_nbt_fields().is_err());
        let mut curve = fixture();
        curve.axes[0] = vec3(0.0, 0.0, 0.0);
        assert!(curve.to_nbt_fields().is_err());
    }
}
