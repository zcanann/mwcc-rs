//! Build-163 integer DAG register allocation.

use super::{DagNode, OpKind};

/// Build-163 integer DAG allocation. Its older scheduler treats r0 as a
/// serial staging lane for the selected sink chain (the return chain, or the
/// last source-level store chain), then lets disjoint chains reuse that lane.
/// A result otherwise flows through its dying operand before falling back to
/// the lowest volatile register. Intervals are closed except at a genuine
/// producer/consumer handoff.
pub fn assign_registers_legacy(
    nodes: &[DagNode],
    order: &[usize],
    params: &[(u32, u8)],
) -> Vec<Option<u8>> {
    let count = nodes.len();
    let mut consumers = vec![Vec::new(); count];
    for (index, node) in nodes.iter().enumerate() {
        for read in &node.reads {
            if let Some(writer) = (0..index)
                .rev()
                .find(|&writer| nodes[writer].writes.contains(read))
            {
                consumers[writer].push(index);
            }
        }
    }
    let mut position = vec![0; count];
    for (slot, &node) in order.iter().enumerate() {
        position[node] = slot;
    }
    let value_end = |node: usize| {
        consumers[node]
            .iter()
            .map(|&reader| position[reader])
            .max()
            .unwrap_or(position[node])
    };
    let param_end = |value: u32| {
        (0..count)
            .filter(|&reader| nodes[reader].reads.contains(&value))
            .map(|reader| position[reader])
            .max()
            .unwrap_or(0)
    };
    let sink_of = |mut node: usize| loop {
        match consumers[node].first() {
            Some(&next) => node = next,
            None => break node,
        }
    };
    let return_node = (0..count).find(|&node| {
        consumers[node].is_empty()
            && nodes[node].kind != OpKind::Store
            && !nodes[node].writes.is_empty()
    });
    let last_store = (0..count)
        .rev()
        .find(|&node| nodes[node].kind == OpKind::Store);
    let return_has_r0_chain = return_node.is_some_and(|return_node| {
        (0..count).any(|node| {
            node != return_node
                && sink_of(node) == return_node
                && !nodes[node].forbid_r0
                && nodes[node].kind != OpKind::Store
                && !nodes[node].writes.is_empty()
        })
    });
    let selected_sink = if return_has_r0_chain {
        return_node
    } else {
        last_store
    };

    let mut result = vec![None; count];
    let mut occupied: Vec<(u8, usize, usize)> = params
        .iter()
        .map(|&(value, register)| (register, 0, param_end(value)))
        .collect();

    // The return's physical result is known before allocating overlapping
    // store chains. The selected chain's r0 intervals are likewise reserved
    // as one serial lane; adjacent members intentionally share their boundary.
    if let Some(return_node) = return_node {
        result[return_node] = Some(3);
        occupied.push((3, position[return_node], value_end(return_node)));
        for node in 0..count {
            if nodes[node].forbid_r0
                && consumers[node].as_slice() == [return_node]
                && result[node].is_none()
            {
                result[node] = Some(3);
                occupied.push((3, position[node], value_end(node)));
            }
        }
    }
    if let Some(selected_sink) = selected_sink {
        for &node in order {
            if Some(node) == return_node
                || nodes[node].kind == OpKind::Store
                || nodes[node].writes.is_empty()
                || nodes[node].forbid_r0
                || sink_of(node) != selected_sink
                || (return_node.is_none() && nodes[node].extension)
            {
                continue;
            }
            result[node] = Some(0);
            occupied.push((0, position[node], value_end(node)));
        }
    }

    for &node in order {
        if result[node].is_some()
            || nodes[node].kind == OpKind::Store
            || nodes[node].writes.is_empty()
        {
            continue;
        }
        let start = position[node];
        let end = value_end(node);
        let own_dying: Vec<u8> = nodes[node]
            .reads
            .iter()
            .filter_map(|read| {
                if let Some(&(value, register)) = params.iter().find(|&&(value, _)| value == *read)
                {
                    return (param_end(value) == start).then_some(register);
                }
                (0..count)
                    .rev()
                    .find(|&writer| nodes[writer].writes.contains(read))
                    .and_then(|writer| {
                        (value_end(writer) == start)
                            .then(|| result[writer])
                            .flatten()
                    })
            })
            .collect();
        let free = |register: u8, open_start: bool| {
            occupied.iter().all(|&(taken, other_start, other_end)| {
                taken != register
                    || other_end < start
                    || other_start > end
                    || (open_start && other_end == start)
            })
        };

        let extension_prefers_home = nodes[node].extension && return_node.is_none();
        let pick_own = || {
            own_dying
                .iter()
                .copied()
                .filter(|&register| register != 0 || !nodes[node].forbid_r0)
                .find(|&register| free(register, true))
        };
        let register = if extension_prefers_home {
            (consumers[node].len() == 1)
                .then(pick_own)
                .flatten()
                .or_else(|| (3..=12).find(|&register| free(register, false)))
        } else {
            let mut candidates = Vec::new();
            if !nodes[node].forbid_r0 && free(0, own_dying.contains(&0)) {
                candidates.push(0);
            }
            candidates.extend((3..=12).filter(|&register| free(register, false)));
            let final_store =
                consumers[node].len() == 1 && nodes[consumers[node][0]].kind == OpKind::Store;
            if return_node.is_some()
                || final_store
                || !nodes[node].forbid_r0
                || nodes[node].extension
                || nodes[node].kind == OpKind::Load
            {
                candidates.extend(pick_own());
            }
            candidates.into_iter().min()
        }
        .unwrap_or(0);
        result[node] = Some(register);
        occupied.push((register, start, end));
    }
    result
}
