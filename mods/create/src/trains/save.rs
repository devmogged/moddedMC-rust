//! TrackGraph/DimensionPalette NBT layout from Create 6.0.10.
//! Graph edges support straight or Bezier rail paths. Signal/point data is
//! not implemented and must be rejected rather than silently erased.

use super::{
    curve::CurveRecord,
    graph::{TrackGraph, TrackNodeLocation},
    math::Vec3,
};
use fastnbt::{IntArray, Value};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const OVERWORLD: &str = "minecraft:overworld";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct DimensionPalette {
    dimensions: Vec<String>,
}

impl DimensionPalette {
    pub fn encode(&mut self, dimension: &str) -> i32 {
        if let Some(index) = self.dimensions.iter().position(|key| key == dimension) {
            return index as i32;
        }
        let index = self.dimensions.len();
        self.dimensions.push(dimension.to_owned());
        index as i32
    }

    pub fn decode(&self, index: i32) -> &str {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.dimensions.get(index))
            .map_or(OVERWORLD, String::as_str)
    }

    #[cfg(test)]
    pub fn to_nbt(&self) -> Result<Vec<u8>, String> {
        fastnbt::to_bytes(&SavedPalette {
            dimensions: self
                .dimensions
                .iter()
                .cloned()
                .map(|id| SavedDimension { id })
                .collect(),
        })
        .map_err(|err| err.to_string())
    }

    #[cfg(test)]
    pub fn from_nbt(bytes: &[u8]) -> Result<Self, String> {
        let palette: SavedPalette = fastnbt::from_bytes(bytes).map_err(|err| err.to_string())?;
        Ok(Self {
            dimensions: palette
                .dimensions
                .into_iter()
                .map(|entry| entry.id)
                .collect(),
        })
    }
}

#[cfg(test)]
#[derive(Serialize, Deserialize)]
struct SavedPalette {
    #[serde(rename = "DimensionPalette")]
    dimensions: Vec<SavedDimension>,
}

#[derive(Serialize, Deserialize)]
struct SavedDimension {
    #[serde(rename = "Id")]
    id: String,
}

#[derive(Serialize, Deserialize)]
struct SavedBlockPos {
    #[serde(rename = "X")]
    x: i32,
    #[serde(rename = "Y")]
    y: i32,
    #[serde(rename = "Z")]
    z: i32,
}

#[derive(Serialize, Deserialize)]
struct SavedLocation {
    #[serde(rename = "Pos")]
    pos: SavedBlockPos,
    #[serde(rename = "D", default)]
    dimension: i32,
    #[serde(rename = "YO", default, skip_serializing_if = "is_zero")]
    y_offset: i32,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

#[derive(Serialize, Deserialize)]
struct SavedNode {
    #[serde(rename = "Location")]
    location: SavedLocation,
    #[serde(rename = "Normal")]
    normal: Vec<f64>,
    #[serde(rename = "Connections", default, skip_serializing_if = "Vec::is_empty")]
    connections: Vec<SavedConnection>,
}

#[derive(Serialize, Deserialize)]
struct SavedConnection {
    #[serde(rename = "To")]
    to: i32,
    #[serde(rename = "EdgeData")]
    edge: SavedEdge,
}

#[derive(Serialize, Deserialize)]
struct SavedEdge {
    #[serde(rename = "Material")]
    material: String,
    #[serde(rename = "Signals", default)]
    signals: HashMap<String, Value>,
    // BezierConnection.write(BlockPos.ZERO) adds Positions/Starts/Axes/Normals.
    // Unknown fields are rejected by CurveRecord; nothing silently discarded.
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

#[derive(Serialize, Deserialize)]
struct SavedGraph {
    #[serde(rename = "Id")]
    id: IntArray,
    #[serde(rename = "Color")]
    color: i32,
    #[serde(rename = "Nodes")]
    nodes: Vec<SavedNode>,
    #[serde(rename = "Points", default)]
    points: HashMap<String, Value>,
}

/// Saves one graph's real NBT compound, not an invented JSON representation.
/// The returned bytes are suitable as the named-root NBT payload of an entry
/// in RailwaySavedData.RailGraphs when WS-0 exposes persistent saved data.
pub(super) fn encode_track_graph(
    graph: &TrackGraph,
    id: [i32; 4],
    color: i32,
    palette: &mut DimensionPalette,
) -> Result<Vec<u8>, String> {
    let mut order: Vec<_> = graph.nodes.keys().cloned().collect();
    order.sort_by(|a, b| {
        (&a.dimension, a.x, a.y, a.z, a.y_offset_pixels).cmp(&(
            &b.dimension,
            b.x,
            b.y,
            b.z,
            b.y_offset_pixels,
        ))
    });
    let indices: HashMap<_, _> = order
        .iter()
        .enumerate()
        .map(|(index, key)| (key.clone(), index as i32))
        .collect();
    let mut nodes = Vec::with_capacity(order.len());
    for key in &order {
        let Some(node) = graph.nodes.get(key) else {
            return Err("missing graph node".into());
        };
        let mut connections = Vec::new();
        if let Some(edges) = graph.connections.get(key) {
            for (other, edge) in edges {
                let Some(to) = indices.get(other) else {
                    return Err("dangling graph edge".into());
                };
                connections.push(SavedConnection {
                    to: *to,
                    edge: SavedEdge {
                        material: edge.material.clone(),
                        signals: HashMap::new(),
                        extra: match &edge.turn {
                            Some(turn) => {
                                let mut fields = turn.record.to_nbt_fields()?;
                                // TrackEdge.write overwrites the turn's Material tag.
                                fields.remove("Material");
                                fields
                            }
                            None => HashMap::new(),
                        },
                    },
                });
            }
        }
        connections.sort_by_key(|connection| connection.to);
        nodes.push(SavedNode {
            location: SavedLocation {
                pos: SavedBlockPos {
                    x: key.x,
                    y: key.y,
                    z: key.z,
                },
                dimension: palette.encode(&key.dimension),
                y_offset: key.y_offset_pixels,
            },
            normal: vec![node.normal.x, node.normal.y, node.normal.z],
            connections,
        });
    }
    fastnbt::to_bytes(&SavedGraph {
        id: IntArray::new(id.to_vec()),
        color,
        nodes,
        points: HashMap::new(),
    })
    .map_err(|err| err.to_string())
}

/// Reject unsupported signals, points, and inconsistent reverse curves.
/// Do not truncate Create data to a straight track when the curve is invalid.
pub(super) fn decode_track_graph(
    bytes: &[u8],
    palette: &DimensionPalette,
) -> Result<(TrackGraph, [i32; 4], i32), String> {
    let saved: SavedGraph = fastnbt::from_bytes(bytes).map_err(|err| err.to_string())?;
    let id = match &*saved.id {
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return Err("invalid graph UUID int array".into()),
    };
    if !saved.points.is_empty() {
        return Err("graph point storage requires M2".into());
    }

    let mut graph = TrackGraph::default();
    let mut locations = Vec::with_capacity(saved.nodes.len());
    for node in &saved.nodes {
        let normal = match node.normal.as_slice() {
            [x, y, z] if x.is_finite() && y.is_finite() && z.is_finite() => Vec3 {
                x: *x,
                y: *y,
                z: *z,
            },
            _ => return Err("invalid node normal".into()),
        };
        let location = TrackNodeLocation {
            dimension: palette.decode(node.location.dimension).to_owned(),
            x: node.location.pos.x,
            y: node.location.pos.y,
            z: node.location.pos.z,
            y_offset_pixels: node.location.y_offset,
        };
        if !graph.create_node_if_absent(location.clone(), normal) {
            return Err("duplicate graph node".into());
        }
        locations.push(location);
    }

    let mut directed = HashMap::new();
    for (from, node) in saved.nodes.iter().enumerate() {
        for link in &node.connections {
            let to = usize::try_from(link.to).map_err(|_| "negative edge index")?;
            if to >= locations.len() || from == to {
                return Err("invalid edge index".into());
            }
            if !link.edge.signals.is_empty() {
                return Err("signal edge requires M2".into());
            }
            let curve = if link.edge.extra.is_empty() {
                None
            } else {
                // TrackEdge.read passes the same compound to BezierConnection.read.
                let mut fields = link.edge.extra.clone();
                fields.insert("Material".into(), Value::String(link.edge.material.clone()));
                Some(CurveRecord::from_nbt_fields(&fields)?)
            };
            if directed
                .insert((from, to), (link.edge.material.as_str(), curve))
                .is_some()
            {
                return Err("duplicate directed edge".into());
            }
        }
    }

    let mut connected = HashSet::new();
    for (&(from, to), (material, curve)) in &directed {
        let Some((reverse_material, reverse_curve)) = directed.get(&(to, from)) else {
            return Err("missing reverse edge".into());
        };
        if material != reverse_material
            || curve.as_ref().map(CurveRecord::secondary) != *reverse_curve
        {
            return Err("mismatched reverse edge or curve".into());
        }
        if connected.insert((from.min(to), from.max(to)))
            && !graph.connect_nodes_with_curve(
                &locations[from],
                &locations[to],
                material,
                curve.clone(),
            )
        {
            return Err("failed to connect graph edge".into());
        }
    }
    Ok((graph, id, saved.color))
}

/// Complete save-global Create RailwaySavedData root. Signal groups, stations and
/// trains are not implemented in M1, so nonempty values are rejected on load.
#[derive(Debug, Default)]
pub(super) struct RailwayData {
    pub graphs: Vec<RailwayGraph>,
}

#[derive(Debug)]
pub(super) struct RailwayGraph {
    pub uuid: [i32; 4],
    pub color: i32,
    pub graph: TrackGraph,
}

#[derive(Serialize, Deserialize)]
struct RailwayNbt {
    #[serde(rename = "RailGraphs", default)]
    rail_graphs: Vec<HashMap<String, Value>>,
    #[serde(rename = "SignalBlocks", default)]
    signal_blocks: Vec<HashMap<String, Value>>,
    #[serde(rename = "Trains", default)]
    trains: Vec<HashMap<String, Value>>,
    #[serde(rename = "DimensionPalette", default)]
    dimensions: Vec<SavedDimension>,
}

impl RailwayData {
    /// RailwaySavedData.save: write RailGraphs, SignalBlocks, Trains and
    /// DimensionPalette. Graph serialization itself reuses TrackGraph.write.
    pub fn to_nbt(&self) -> Result<Vec<u8>, String> {
        let mut palette = DimensionPalette::default();
        let mut graphs = Vec::with_capacity(self.graphs.len());
        let mut seen = HashSet::new();
        for network in &self.graphs {
            if !seen.insert(network.uuid) {
                return Err("duplicate railway graph UUID".into());
            }
            let bytes =
                encode_track_graph(&network.graph, network.uuid, network.color, &mut palette)?;
            graphs.push(fastnbt::from_bytes(&bytes).map_err(|err| err.to_string())?);
        }
        fastnbt::to_bytes(&RailwayNbt {
            rail_graphs: graphs,
            signal_blocks: Vec::new(),
            trains: Vec::new(),
            dimensions: palette
                .dimensions
                .into_iter()
                .map(|id| SavedDimension { id })
                .collect(),
        })
        .map_err(|err| err.to_string())
    }

    /// RailwaySavedData.load: decode dimension palette before graphs; reject
    /// future milestone data rather than silently discarding it on next save.
    pub fn from_nbt(bytes: &[u8]) -> Result<Self, String> {
        let data: RailwayNbt = fastnbt::from_bytes(bytes).map_err(|err| err.to_string())?;
        if !data.signal_blocks.is_empty() {
            return Err("signal groups require M2".into());
        }
        if !data.trains.is_empty() {
            return Err("trains require M3".into());
        }
        let palette = DimensionPalette {
            dimensions: data.dimensions.into_iter().map(|row| row.id).collect(),
        };
        let mut seen = HashSet::new();
        let mut graphs = Vec::with_capacity(data.rail_graphs.len());
        for raw in data.rail_graphs {
            let bytes = fastnbt::to_bytes(&raw).map_err(|err| err.to_string())?;
            let (graph, uuid, color) = decode_track_graph(&bytes, &palette)?;
            if !seen.insert(uuid) {
                return Err("duplicate railway graph UUID".into());
            }
            graphs.push(RailwayGraph { uuid, color, graph });
        }
        Ok(Self { graphs })
    }

    pub fn total_nodes(&self) -> usize {
        self.graphs.iter().map(|g| g.graph.nodes.len()).sum()
    }

    pub fn total_directed_edges(&self) -> usize {
        self.graphs
            .iter()
            .flat_map(|g| g.graph.connections.values())
            .map(HashMap::len)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(dimension: &str, x: f64, y: f64, z: f64) -> TrackNodeLocation {
        TrackNodeLocation::from_world(dimension, Vec3 { x, y, z }, 0)
    }

    fn graph_fixture() -> (TrackGraph, TrackNodeLocation, TrackNodeLocation) {
        let (a, b, c) = (
            pos(OVERWORLD, 0.5, 64.0, 0.5),
            pos(OVERWORLD, 8.5, 64.0, 0.5),
            pos("minecraft:the_nether", 0.5, 64.0, 0.5),
        );
        let mut graph = TrackGraph::default();
        let up = Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        graph.create_node_if_absent(a.clone(), up);
        graph.create_node_if_absent(b.clone(), up);
        graph.create_node_if_absent(c, up);
        assert!(graph.connect_nodes(&a, &b, "create:andesite"));
        (graph, a, b)
    }

    #[test]
    fn railway_saved_data_matches_java_root_and_preserves_graphs_and_dimensions() {
        let (first, a, b) = graph_fixture();
        let (curved, c, d) = curve_fixture();
        let data = RailwayData {
            graphs: vec![
                RailwayGraph {
                    uuid: [1, 2, 3, 4],
                    color: 0x445566,
                    graph: first,
                },
                RailwayGraph {
                    uuid: [5, 6, 7, 8],
                    color: 0x112233,
                    graph: curved,
                },
            ],
        };
        let bytes = data.to_nbt().unwrap();
        let raw: RailwayNbt = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(raw.rail_graphs.len(), 2);
        assert!(raw.signal_blocks.is_empty());
        assert!(raw.trains.is_empty());
        assert_eq!(raw.dimensions[0].id, OVERWORLD);
        assert_eq!(raw.dimensions[1].id, "minecraft:the_nether");
        let loaded = RailwayData::from_nbt(&bytes).unwrap();
        assert_eq!(loaded.graphs.len(), 2);
        assert_eq!(loaded.total_nodes(), 5);
        assert_eq!(loaded.total_directed_edges(), 4);
        assert_eq!(loaded.graphs[0].color, 0x445566);
        assert_eq!(
            loaded.graphs[0].graph.connections[&a][&b].material,
            "create:andesite"
        );
        assert!(loaded.graphs[1].graph.connections[&c][&d].turn.is_some());
        // HashMap iteration changes NBT compound field order; compare the
        // serialized meaning, not bytes for unordered Java CompoundTags.
        let twice = RailwayData::from_nbt(&loaded.to_nbt().unwrap()).unwrap();
        assert_eq!(twice.total_nodes(), loaded.total_nodes());
        assert_eq!(twice.total_directed_edges(), loaded.total_directed_edges());
        assert_eq!(twice.graphs[1].uuid, [5, 6, 7, 8]);
    }

    #[test]
    fn railway_saved_data_rejects_unsupported_records_and_duplicate_ids() {
        let (graph, _, _) = graph_fixture();
        let duplicate = RailwayData {
            graphs: vec![
                RailwayGraph {
                    uuid: [7; 4],
                    color: 0,
                    graph,
                },
                RailwayGraph {
                    uuid: [7; 4],
                    color: 1,
                    graph: TrackGraph::default(),
                },
            ],
        };
        assert!(duplicate.to_nbt().unwrap_err().contains("duplicate"));
        let mut root: RailwayNbt =
            fastnbt::from_bytes(&RailwayData::default().to_nbt().unwrap()).unwrap();
        root.trains
            .push(HashMap::from([("Id".into(), Value::Int(3))]));
        let bytes = fastnbt::to_bytes(&root).unwrap();
        assert!(
            RailwayData::from_nbt(&bytes)
                .unwrap_err()
                .contains("trains")
        );
        root.trains.clear();
        root.signal_blocks
            .push(HashMap::from([("Id".into(), Value::Int(3))]));
        let bytes = fastnbt::to_bytes(&root).unwrap();
        assert!(
            RailwayData::from_nbt(&bytes)
                .unwrap_err()
                .contains("signal")
        );
        assert!(RailwayData::from_nbt(&[1, 2, 3]).is_err());
    }

    #[test]
    fn dimension_palette_nbt_preserves_order_and_overworld_fallback() {
        let mut palette = DimensionPalette::default();
        assert_eq!(palette.encode(OVERWORLD), 0);
        assert_eq!(palette.encode("minecraft:the_nether"), 1);
        assert_eq!(palette.encode(OVERWORLD), 0);
        assert_eq!(palette.decode(1), "minecraft:the_nether");
        assert_eq!(palette.decode(-1), OVERWORLD);
        assert_eq!(palette.decode(999), OVERWORLD);
        let bytes = palette.to_nbt().unwrap();
        let loaded = DimensionPalette::from_nbt(&bytes).unwrap();
        assert_eq!(loaded, palette);
        let nbt: SavedPalette = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(nbt.dimensions[0].id, OVERWORLD);
        assert_eq!(nbt.dimensions[1].id, "minecraft:the_nether");
    }

    #[test]
    fn straight_graph_nbt_roundtrips_java_keys_palette_and_both_edges() {
        let (graph, a, b) = graph_fixture();
        let mut palette = DimensionPalette::default();
        let id = [0x12, 0x34, -1, 0x42];
        let bytes = encode_track_graph(&graph, id, 0x52_66_88, &mut palette).unwrap();
        let saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(&*saved.id, &id);
        assert_eq!(saved.nodes.len(), 3);
        assert!(saved.points.is_empty());
        assert_eq!(
            saved
                .nodes
                .iter()
                .map(|node| node.connections.len())
                .sum::<usize>(),
            2
        );
        assert_eq!(palette.decode(1), "minecraft:the_nether");

        let (loaded, loaded_id, color) = decode_track_graph(&bytes, &palette).unwrap();
        assert_eq!(loaded_id, id);
        assert_eq!(color, 0x52_66_88);
        assert_eq!(loaded.nodes.len(), graph.nodes.len());
        assert_eq!(loaded.connections[&a][&b].material, "create:andesite");
        assert_eq!(loaded.connections[&b][&a].material, "create:andesite");
        assert_eq!(loaded.nodes[&a].normal, graph.nodes[&a].normal);
        assert_eq!(loaded.connected_components().len(), 2);
    }

    #[test]
    fn decoding_rejects_asymmetric_or_corrupted_connections() {
        let (graph, _, _) = graph_fixture();
        let mut palette = DimensionPalette::default();
        let bytes = encode_track_graph(&graph, [0; 4], 0, &mut palette).unwrap();
        let mut saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        let connected_indices: Vec<_> = saved
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| !node.connections.is_empty())
            .map(|(index, _)| index)
            .collect();
        assert_eq!(connected_indices.len(), 2);
        saved.nodes[connected_indices[0]].connections.clear();
        let bad = fastnbt::to_bytes(&saved).unwrap();
        assert!(
            decode_track_graph(&bad, &palette)
                .unwrap_err()
                .contains("reverse")
        );

        let mut saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        saved.nodes[connected_indices[0]].connections[0].to = 99;
        let bad = fastnbt::to_bytes(&saved).unwrap();
        assert!(
            decode_track_graph(&bad, &palette)
                .unwrap_err()
                .contains("index")
        );
    }

    #[test]
    fn malformed_curve_and_signal_data_fail_instead_of_being_dropped() {
        let (graph, _, _) = graph_fixture();
        let mut palette = DimensionPalette::default();
        let bytes = encode_track_graph(&graph, [0; 4], 0, &mut palette).unwrap();
        let mut saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        let source = saved
            .nodes
            .iter_mut()
            .find(|node| !node.connections.is_empty())
            .unwrap();
        source.connections[0]
            .edge
            .extra
            .insert("Positions".into(), Value::List(Vec::new()));
        let bad = fastnbt::to_bytes(&saved).unwrap();
        assert!(
            decode_track_graph(&bad, &palette)
                .unwrap_err()
                .contains("missing")
        );

        let mut saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        saved.points.insert("create:station".into(), Value::Int(4));
        let bad = fastnbt::to_bytes(&saved).unwrap();
        assert!(
            decode_track_graph(&bad, &palette)
                .unwrap_err()
                .contains("point")
        );
    }

    #[test]
    fn location_y_offset_is_encoded_as_optional_yo() {
        let (mut graph, _, _) = graph_fixture();
        let raised = TrackNodeLocation {
            dimension: OVERWORLD.to_owned(),
            x: 65,
            y: 128,
            z: 0,
            y_offset_pixels: 7,
        };
        graph.create_node_if_absent(
            raised.clone(),
            Vec3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
        );
        let mut palette = DimensionPalette::default();
        let bytes = encode_track_graph(&graph, [0; 4], 0, &mut palette).unwrap();
        let saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        let special = saved
            .nodes
            .iter()
            .find(|node| node.location.pos.x == 65)
            .unwrap();
        assert_eq!(special.location.y_offset, 7);
        assert_eq!(special.location.pos.y, 128);
        let (loaded, ..) = decode_track_graph(&bytes, &palette).unwrap();
        assert!(loaded.nodes.contains_key(&raised));
    }

    fn curve_fixture() -> (TrackGraph, TrackNodeLocation, TrackNodeLocation) {
        let a = pos(OVERWORLD, 0.5, 64.0, 0.5);
        let b = pos(OVERWORLD, 8.5, 64.0, 8.5);
        let up = Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        let mut graph = TrackGraph::default();
        graph.create_node_if_absent(a.clone(), up);
        graph.create_node_if_absent(b.clone(), up);
        let turn = CurveRecord {
            block_positions: [[0, 64, 0], [8, 64, 8]],
            starts: [a.world(), b.world()],
            axes: [
                Vec3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: -1.0,
                },
            ],
            normals: [up, up],
            material: "create:brass".to_owned(),
            primary: true,
            girder: false,
            smoothing: Some([3, 0]),
        };
        assert!(graph.connect_nodes_with_curve(&a, &b, "create:brass", Some(turn)));
        (graph, a, b)
    }

    #[test]
    fn curved_graph_nbt_roundtrips_bezier_in_both_directions() {
        let (graph, a, b) = curve_fixture();
        let mut palette = DimensionPalette::default();
        let bytes = encode_track_graph(&graph, [1, 2, 3, 4], -1, &mut palette).unwrap();
        let saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(saved.nodes.len(), 2);
        for node in &saved.nodes {
            assert_eq!(node.connections.len(), 1);
            let edge = &node.connections[0].edge;
            assert_eq!(edge.material, "create:brass");
            let curve = &edge.extra;
            assert!(!curve.contains_key("Material"));
            for key in [
                "Positions",
                "Starts",
                "Axes",
                "Normals",
                "Primary",
                "Girder",
                "Smoothing",
            ] {
                assert!(curve.contains_key(key), "missing {key}");
            }
        }
        let (loaded, id, color) = decode_track_graph(&bytes, &palette).unwrap();
        assert_eq!(id, [1, 2, 3, 4]);
        assert_eq!(color, -1);
        let first = &loaded.connections[&a][&b];
        let reverse = &loaded.connections[&b][&a];
        assert!(first.turn.is_some());
        assert!(reverse.turn.is_some());
        assert!(first.turn.as_ref().unwrap().record.primary);
        assert_eq!(first.turn.as_ref().unwrap().record.material, "create:brass");
        assert_eq!(
            reverse.turn.as_ref().unwrap().record.material,
            "create:brass"
        );
        assert!(!reverse.turn.as_ref().unwrap().record.primary);
        assert_eq!(first.turn.as_ref().unwrap().record.smoothing, Some([3, 0]));
        assert_eq!(
            reverse.turn.as_ref().unwrap().record.smoothing,
            Some([0, 3])
        );
        assert!((first.length() - reverse.length()).abs() < 1.0e-8);
        assert!((first.position(0.5).distance(reverse.position(0.5))) < 1.0e-8);
    }

    #[test]
    fn rejects_curve_nbt_with_mismatched_reverse_primary() {
        let (graph, _, _) = curve_fixture();
        let mut palette = DimensionPalette::default();
        let bytes = encode_track_graph(&graph, [0; 4], 0, &mut palette).unwrap();
        let mut saved: SavedGraph = fastnbt::from_bytes(&bytes).unwrap();
        let reverse = &mut saved.nodes[1].connections[0].edge.extra;
        let value = reverse.get_mut("Primary").unwrap();
        *value = Value::Byte(1);
        let bad = fastnbt::to_bytes(&saved).unwrap();
        assert!(
            decode_track_graph(&bad, &palette)
                .unwrap_err()
                .contains("mismatched reverse edge")
        );
    }
}
