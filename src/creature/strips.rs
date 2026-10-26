//! Connected strips for native packets. Vertex identities include their UV references, so
//! connecting an edge never merges a texture seam. Construction happens when loading a model.

use std::collections::HashMap;

type Triangle = [u16; 3];

fn degenerate(t: Triangle) -> bool {
    t[0] == t[1] || t[1] == t[2] || t[2] == t[0]
}

fn next_edge(vertices: &[u16]) -> (u16, u16) {
    let n = vertices.len();
    if n.is_multiple_of(2) {
        (vertices[n - 2], vertices[n - 1])
    } else {
        (vertices[n - 1], vertices[n - 2])
    }
}

fn third(t: Triangle, edge: (u16, u16)) -> Option<u16> {
    (0..3).find_map(|i| (t[i] == edge.0 && t[(i + 1) % 3] == edge.1).then_some(t[(i + 2) % 3]))
}

/// Preserve each oriented source face, including duplicate faces and exact vertex indices.
/// Transparent geometry keeps its source face order; opaque geometry may connect any unused
/// oriented edge. Degenerate source faces remain independent, harmless three-vertex strips.
/// An incomplete final face is ignored; the packet encoder validates divisibility by three.
pub(crate) fn strips(triangles: &[u16], reorder: bool) -> Vec<Vec<u16>> {
    let faces: Vec<Triangle> = triangles.as_chunks::<3>().0.to_vec();
    if !reorder {
        return ordered(&faces);
    }
    connected(&faces)
}

fn ordered(faces: &[Triangle]) -> Vec<Vec<u16>> {
    let mut output: Vec<Vec<u16>> = Vec::new();
    for &face in faces {
        if !degenerate(face)
            && let Some(last) = output.last_mut()
            && !degenerate([last[0], last[1], last[2]])
        {
            if let Some(vertex) = third(face, next_edge(last)) {
                last.push(vertex);
                continue;
            }
            // A fresh strip can rotate its first triangle without changing its winding or
            // the source face order. Choose the first rotation that connects the next face.
            if last.len() == 3 {
                let first = [last[0], last[1], last[2]];
                let mut joined = false;
                for rotation in 1..3 {
                    let rotated = [
                        first[rotation],
                        first[(rotation + 1) % 3],
                        first[(rotation + 2) % 3],
                    ];
                    if let Some(vertex) = third(face, (rotated[2], rotated[1])) {
                        last.copy_from_slice(&rotated);
                        last.push(vertex);
                        joined = true;
                        break;
                    }
                }
                if joined {
                    continue;
                }
            }
        }
        output.push(face.to_vec());
    }
    output
}

struct Edges {
    index: HashMap<(u16, u16), usize>,
    lists: Vec<Vec<(usize, u16)>>,
    // Each cursor skips only faces already committed to the output. Trial cursors also skip
    // faces visited in that trial, without making other starting rotations lose those faces.
    committed: Vec<usize>,
    trial_cursor: Vec<usize>,
    trial_generation: Vec<usize>,
}

impl Edges {
    fn new(faces: &[Triangle]) -> Self {
        let mut index = HashMap::new();
        let mut lists: Vec<Vec<(usize, u16)>> = Vec::new();
        for (face, &t) in faces.iter().enumerate() {
            if degenerate(t) {
                continue;
            }
            for i in 0..3 {
                let edge = (t[i], t[(i + 1) % 3]);
                let next = lists.len();
                let list = *index.entry(edge).or_insert_with(|| {
                    lists.push(Vec::new());
                    next
                });
                lists[list].push((face, t[(i + 2) % 3]));
            }
        }
        let n = lists.len();
        Self {
            index,
            lists,
            committed: vec![0; n],
            trial_cursor: vec![0; n],
            trial_generation: vec![0; n],
        }
    }

    fn take(
        &mut self,
        edge: (u16, u16),
        used: &[bool],
        visited: &[usize],
        generation: usize,
    ) -> Option<(usize, u16)> {
        let &list = self.index.get(&edge)?;
        let entries = &self.lists[list];
        while self.committed[list] < entries.len() && used[entries[self.committed[list]].0] {
            self.committed[list] += 1;
        }
        if self.trial_generation[list] != generation {
            self.trial_generation[list] = generation;
            self.trial_cursor[list] = self.committed[list];
        }
        let cursor = &mut self.trial_cursor[list];
        while *cursor < entries.len() {
            let next = entries[*cursor];
            *cursor += 1;
            if !used[next.0] && visited[next.0] != generation {
                return Some(next);
            }
        }
        None
    }
}

fn connected(faces: &[Triangle]) -> Vec<Vec<u16>> {
    let mut edges = Edges::new(faces);
    let mut used = vec![false; faces.len()];
    let mut visited = vec![0; faces.len()];
    let mut generation = 0;
    let mut output = Vec::new();
    for seed in 0..faces.len() {
        if used[seed] {
            continue;
        }
        let face = faces[seed];
        if degenerate(face) {
            used[seed] = true;
            output.push(face.to_vec());
            continue;
        }
        let mut best = Vec::new();
        let mut best_faces = Vec::new();
        // Three deterministic trials choose a useful starting edge. Each trial visits a
        // face at most once and advances edge cursors monotonically; no search or recursion.
        for rotation in 0..3 {
            generation += 1;
            let mut vertices = vec![
                face[rotation],
                face[(rotation + 1) % 3],
                face[(rotation + 2) % 3],
            ];
            let mut selected = vec![seed];
            visited[seed] = generation;
            while let Some((face, vertex)) =
                edges.take(next_edge(&vertices), &used, &visited, generation)
            {
                visited[face] = generation;
                selected.push(face);
                vertices.push(vertex);
            }
            if vertices.len() > best.len() {
                best = vertices;
                best_faces = selected;
            }
        }
        for face in best_faces {
            used[face] = true;
        }
        output.push(best);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn oriented(t: Triangle) -> Triangle {
        [t, [t[1], t[2], t[0]], [t[2], t[0], t[1]]]
            .into_iter()
            .min()
            .unwrap()
    }

    fn source(indices: &[u16]) -> Vec<Triangle> {
        indices
            .as_chunks::<3>()
            .0
            .iter()
            .copied()
            .filter(|&t| !degenerate(t))
            .map(oriented)
            .collect()
    }

    fn decoded(strips: &[Vec<u16>]) -> Vec<Triangle> {
        strips
            .iter()
            .flat_map(|s| {
                s.windows(3).enumerate().filter_map(|(i, w)| {
                    let t = if i.is_multiple_of(2) {
                        [w[0], w[1], w[2]]
                    } else {
                        [w[1], w[0], w[2]]
                    };
                    (!degenerate(t)).then(|| oriented(t))
                })
            })
            .collect()
    }

    fn multiset(faces: Vec<Triangle>) -> BTreeMap<Triangle, usize> {
        let mut counts = BTreeMap::new();
        for face in faces {
            *counts.entry(face).or_default() += 1;
        }
        counts
    }

    #[test]
    fn connected_faces_use_both_strip_windings_without_changing_source_faces() {
        let input = [0, 1, 2, 2, 1, 3, 2, 3, 4, 4, 3, 5];
        for reorder in [false, true] {
            let result = strips(&input, reorder);
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].len(), 6);
            assert_eq!(decoded(&result), source(&input));
        }
    }

    #[test]
    fn transparent_faces_keep_their_exact_sequence_across_disconnected_geometry() {
        let input = [1, 2, 0, 8, 9, 10, 1, 3, 2, 4, 2, 3, 4, 3, 5];
        let result = strips(&input, false);
        assert_eq!(decoded(&result), source(&input));
        let opaque = strips(&input, true);
        assert_eq!(multiset(decoded(&opaque)), multiset(source(&input)));
        assert!(opaque.len() < result.len());
    }

    #[test]
    fn seams_duplicate_faces_opposite_winding_and_degenerates_keep_their_identity() {
        let input = [
            0, 1, 2, 0, 1, 2, 0, 2, 1, 10, 11, 12, 12, 11, 13, 10, 10, 11, 9, 9, 9, 65_533, 65_534,
            65_535,
        ];
        for reorder in [false, true] {
            let result = strips(&input, reorder);
            assert_eq!(multiset(decoded(&result)), multiset(source(&input)));
            assert_eq!(
                result
                    .iter()
                    .filter(|s| degenerate([s[0], s[1], s[2]]))
                    .count(),
                2
            );
            if !reorder {
                assert_eq!(decoded(&result), source(&input));
            }
            assert_eq!(result, strips(&input, reorder));
        }
    }

    #[test]
    fn shuffled_packet_grid_reconnects_without_losing_or_reversing_faces() {
        let mut faces = Vec::new();
        for y in 0..15u16 {
            for x in 0..15u16 {
                let a = y * 16 + x;
                faces.push([a, a + 1, a + 16]);
                faces.push([a + 16, a + 1, a + 17]);
            }
        }
        let input: Vec<u16> = (0..faces.len())
            .flat_map(|i| faces[(i * 137) % faces.len()])
            .collect();
        let result = strips(&input, true);
        assert_eq!(multiset(decoded(&result)), multiset(source(&input)));
        assert!(result.iter().map(Vec::len).sum::<usize>() < input.len() / 2);
        assert_eq!(result, strips(&input, true));
        assert_eq!(decoded(&strips(&input, false)), source(&input));
    }

    #[test]
    fn large_duplicate_packet_is_bounded_and_empty_or_partial_input_is_harmless() {
        let input: Vec<u16> = (0..6000).flat_map(|_| [1, 2, 3]).collect();
        let result = strips(&input, true);
        assert_eq!(result.len(), 6000);
        assert_eq!(multiset(decoded(&result)), multiset(source(&input)));
        for reorder in [false, true] {
            assert!(strips(&[], reorder).is_empty());
            assert!(strips(&[1, 2], reorder).is_empty());
            assert_eq!(strips(&[1, 2, 3, 4], reorder), vec![vec![1, 2, 3]]);
        }
    }
}
