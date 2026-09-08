//! Reverse interval coloring around explicit physical staging registers.

use super::DagNode;

/// Reserve the supplied node homes, then color remaining values in reverse
/// emission order. Closed intervals allow only genuine producer/consumer
/// handoffs to share an instruction boundary. Parameter homes stay closed.
/// Invalid fixed assignments or exhausted pools return `None`, never an
/// arbitrary register that could overwrite a live value.
pub fn assign_registers_reverse_with_fixed(
    nodes: &[DagNode],
    order: &[usize],
    params: &[(u32, u8)],
    fixed: &[(usize, u8)],
    pool: &[u8],
) -> Option<Vec<Option<u8>>> {
    let mut position = vec![usize::MAX; nodes.len()];
    if order.len() != nodes.len() {
        return None;
    }
    for (slot, &node) in order.iter().enumerate() {
        if node >= nodes.len() || position[node] != usize::MAX {
            return None;
        }
        position[node] = slot;
    }
    let end_of = |values: &[u32], start: usize| {
        nodes
            .iter()
            .enumerate()
            .filter(|(_, reader)| reader.reads.iter().any(|value| values.contains(value)))
            .map(|(reader, _)| position[reader])
            .max()
            .unwrap_or(start)
    };
    let mut occupied: Vec<(u8, usize, usize, Option<usize>)> = params
        .iter()
        .map(|&(value, register)| (register, 0, end_of(&[value], 0), None))
        .collect();
    let mut result = vec![None; nodes.len()];
    let handoff = |producer: usize, consumer: usize| {
        nodes[producer]
            .writes
            .iter()
            .any(|value| nodes[consumer].reads.contains(value))
    };
    let available = |node: usize, register: u8, occupied: &[(u8, usize, usize, Option<usize>)]| {
        let start = position[node];
        let end = end_of(&nodes[node].writes, start);
        !(register == 0 && nodes[node].forbid_r0)
            && occupied.iter().all(|&(taken, from, to, owner)| {
                taken != register
                    || to < start
                    || from > end
                    || owner.is_some_and(|other| {
                        (to == start && handoff(other, node))
                            || (end == from && handoff(node, other))
                    })
            })
    };
    for &(node, register) in fixed {
        if node >= nodes.len()
            || nodes[node].writes.is_empty()
            || result[node].is_some()
            || !available(node, register, &occupied)
        {
            return None;
        }
        result[node] = Some(register);
        occupied.push((
            register,
            position[node],
            end_of(&nodes[node].writes, position[node]),
            Some(node),
        ));
    }
    for &node in order.iter().rev() {
        if nodes[node].writes.is_empty() || result[node].is_some() {
            continue;
        }
        let register = pool
            .iter()
            .copied()
            .find(|&register| available(node, register, &occupied))?;
        result[node] = Some(register);
        occupied.push((
            register,
            position[node],
            end_of(&nodes[node].writes, position[node]),
            Some(node),
        ));
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_overlapping_fixed_inputs_but_allows_the_result_handoff() {
        let nodes = [
            DagNode::new("left", 2).writes(&[1]),
            DagNode::new("right", 2).writes(&[2]),
            DagNode::new("sum", 1).reads(&[1, 2]).writes(&[3]),
            DagNode::new("store", 1).reads(&[3]),
        ];
        assert!(assign_registers_reverse_with_fixed(
            &nodes,
            &[0, 1, 2, 3],
            &[],
            &[(0, 0), (1, 0)],
            &[4]
        )
        .is_none());
        assert_eq!(
            assign_registers_reverse_with_fixed(
                &nodes,
                &[0, 1, 2, 3],
                &[],
                &[(1, 0), (2, 0)],
                &[4]
            ),
            Some(vec![Some(4), Some(0), Some(0), None])
        );
    }

    #[test]
    fn reverse_coloring_reuses_disjoint_homes_and_reports_exhaustion() {
        let nodes = [
            DagNode::new("first", 2).writes(&[1]),
            DagNode::new("second", 2).writes(&[2]),
            DagNode::new("consume first", 1).reads(&[1]),
            DagNode::new("third", 2).writes(&[3]),
            DagNode::new("consume second", 1).reads(&[2]),
            DagNode::new("consume third", 1).reads(&[3]),
        ];
        let order = [0, 1, 2, 3, 4, 5];
        assert_eq!(
            assign_registers_reverse_with_fixed(&nodes, &order, &[], &[], &[4, 5]),
            Some(vec![Some(4), Some(5), None, Some(4), None, None])
        );
        assert!(assign_registers_reverse_with_fixed(&nodes, &order, &[], &[], &[4]).is_none());
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    use crate::{linearize_with_ordering, Model, OpKind, FROZEN};

    #[test]
    fn physical_staging_preserves_independent_load_priority() {
        let mut nodes = Vec::new();
        for i in 0..3 {
            nodes.push(DagNode::new("word", 2).writes(&[3 * i + 1]));
            nodes.push(DagNode::new("half", 2).reads(&[0]).writes(&[3 * i + 2]));
            nodes.push(
                DagNode::new("add", 1)
                    .reads(&[3 * i + 1, 3 * i + 2])
                    .writes(&[3 * i + 3]),
            );
            nodes.push(
                DagNode::new("store", 1)
                    .kind(OpKind::Store)
                    .reads(&[3 * i + 3]),
            );
        }
        let model = Model {
            weight_before_kind: false,
            ..FROZEN
        };
        assert_eq!(
            linearize_with_ordering(&nodes, model, &[(3, 5, 1), (7, 9, 1)]),
            [0, 1, 4, 2, 8, 3, 5, 6, 7, 9, 10, 11]
        );
        assert_eq!(
            linearize_with_ordering(
                &nodes,
                Model {
                    issue_width: 1,
                    ..model
                },
                &[(3, 5, 2), (7, 9, 2)]
            ),
            [0, 1, 4, 2, 3, 8, 5, 6, 7, 9, 10, 11]
        );
    }
}
