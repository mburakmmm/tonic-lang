//! Stop-the-world generational tracing and compaction. Minor collections trace
//! the nursery from precise roots plus remembered old owners; periodic major
//! collections trace the full heap. Foreign payload destruction is extracted
//! into a deferred queue and never runs during collector bookkeeping.
use super::{invalid_value, Heap};
use crate::value::Value;
use tonic_core::diagnostic::Result;

#[derive(Clone, Copy, Debug, Default)]
pub struct CollectionStats {
    pub reclaimed: usize,
    pub young_reclaimed: usize,
    pub survivors: usize,
    pub promoted: usize,
    pub moved: usize,
    pub live_bytes: usize,
    pub major: bool,
}
impl Heap {
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) -> Result<CollectionStats> {
        let mut marked = vec![false; self.objects.len()];
        let mut work: Vec<Value> = roots.into_iter().collect();
        // Validate and mark completely before modifying slots; failure is atomic.
        while let Some(value) = work.pop() {
            if value.heap_index().is_none() {
                continue;
            }
            let location = self.location(value).ok_or_else(|| invalid_value(value))?;
            if marked[location] {
                continue;
            }
            marked[location] = true;
            self.objects[location]
                .object
                .trace(|value| work.push(value));
        }
        Ok(self.sweep(marked, false))
    }

    pub fn collect_young(
        &mut self,
        roots: impl IntoIterator<Item = Value>,
    ) -> Result<CollectionStats> {
        let mut marked = vec![false; self.objects.len()];
        let mut work: Vec<Value> = roots.into_iter().collect();
        for slot_index in self.remembered.iter().copied() {
            let Some(location) = self
                .slots
                .get(slot_index as usize)
                .and_then(|slot| slot.location)
            else {
                continue;
            };
            self.objects[location]
                .object
                .trace(|value| work.push(value));
        }
        // Validate every external root, but traverse only nursery objects. Any
        // old-to-young edge must arrive through the remembered owner set.
        while let Some(value) = work.pop() {
            if value.heap_index().is_none() {
                continue;
            }
            let location = self.location(value).ok_or_else(|| invalid_value(value))?;
            if !self.objects[location].young || marked[location] {
                continue;
            }
            marked[location] = true;
            self.objects[location]
                .object
                .trace(|value| work.push(value));
        }
        Ok(self.sweep(marked, true))
    }

    fn sweep(&mut self, marked: Vec<bool>, young_only: bool) -> CollectionStats {
        let mut stats = CollectionStats {
            major: !young_only,
            ..CollectionStats::default()
        };
        let mut old_location = 0;
        let slots = &mut self.slots;
        let free = &mut self.free;
        let pending_foreign = &mut self.pending_foreign;
        self.objects.retain_mut(|entry| {
            let live = (!entry.young && young_only) || marked[old_location];
            let slot = &mut slots[entry.slot as usize];
            if live {
                if old_location != stats.survivors {
                    stats.moved += 1;
                }
                if entry.young {
                    stats.promoted += 1;
                    entry.young = false;
                }
                slot.location = Some(stats.survivors);
                stats.survivors += 1;
                stats.live_bytes += entry.object.estimated_bytes();
            } else {
                if let super::Object::Foreign(foreign) = &mut entry.object {
                    pending_foreign.push(foreign.take_finalizer());
                }
                slot.location = None;
                stats.reclaimed += 1;
                if entry.young {
                    stats.young_reclaimed += 1;
                }
                // Retire exhausted slots permanently. Generation never wraps.
                if slot.generation < Value::MAX_GENERATION {
                    slot.generation += 1;
                    free.push(entry.slot);
                }
            }
            old_location += 1;
            live
        });
        self.bytes = stats.live_bytes;
        self.last_collection_allocations = self.allocations;
        self.remembered.clear();
        let slots = &self.slots;
        for entry in &mut self.objects {
            let super::Object::Class(class) = &mut entry.object else {
                continue;
            };
            class.dependents.retain(|value| {
                value
                    .heap_index()
                    .and_then(|index| slots.get(index))
                    .is_some_and(|slot| {
                        slot.generation == value.generation() && slot.location.is_some()
                    })
            });
        }
        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Object;
    #[test]
    fn cycle_collection_and_stale_generation() {
        let mut heap = Heap::default();
        let a = heap.alloc(Object::List(vec![])).unwrap();
        heap.append_list(a, a).unwrap();
        let b = heap.alloc(Object::Str("kept".into())).unwrap();
        let stats = heap.collect([b]).unwrap();
        assert_eq!(stats.reclaimed, 1);
        assert_eq!(stats.moved, 1);
        assert!(matches!(heap.get(b),Ok(Object::Str(s)) if s=="kept"));
        let c = heap.alloc(Object::Str("new".into())).unwrap();
        assert_eq!(a.heap_index(), c.heap_index());
        assert_ne!(a, c);
        assert!(heap.get(a).is_err());
    }
    #[test]
    fn cells_functions_and_defaults_trace() {
        let mut heap = Heap::default();
        let dead = heap.alloc(Object::Str("dead".into())).unwrap();
        let value = heap.alloc(Object::Str("value".into())).unwrap();
        let cell = heap.alloc(Object::Cell(value)).unwrap();
        let default = heap.alloc(Object::List(vec![value])).unwrap();
        let f = heap
            .alloc(Object::Function {
                code: 1,
                execution: 1,
                captures: vec![cell],
                defaults: vec![default],
            })
            .unwrap();
        let stats = heap.collect([f]).unwrap();
        assert_eq!(stats.reclaimed, 1);
        assert!(heap.get(dead).is_err());
        assert_eq!(heap.cell(cell).unwrap(), value);
        assert_eq!(stats.survivors, 4);
    }
    #[test]
    fn invalid_roots_do_not_partially_collect() {
        let mut heap = Heap::default();
        let value = heap.alloc(Object::Str("valid".into())).unwrap();
        assert!(heap.collect([value, Value::heap(100, 1)]).is_err());
        assert!(heap.get(value).is_ok());
        assert_eq!(heap.live_objects(), 1);
    }
    #[test]
    fn generation_exhaustion_retires_slot() {
        let mut heap = Heap::default();
        let old = heap.alloc(Object::Str("old".into())).unwrap();
        heap.slots[old.heap_index().unwrap()].generation = Value::MAX_GENERATION;
        heap.collect([]).unwrap();
        let new = heap.alloc(Object::Str("new".into())).unwrap();
        assert_ne!(old.heap_index(), new.heap_index());
    }
    #[test]
    fn mutation_after_collection_keeps_new_edges() {
        let mut heap = Heap::default();
        let list = heap.alloc(Object::List(vec![])).unwrap();
        heap.collect([list]).unwrap();
        let young = heap.alloc(Object::Str("young".into())).unwrap();
        heap.append_list(list, young).unwrap();
        heap.collect([list]).unwrap();
        assert!(heap.get(young).is_ok());
    }
    #[test]
    fn minor_collects_nursery_promotes_survivors_and_major_reclaims_old() {
        let mut heap = Heap::default();
        let live = heap.alloc(Object::List(vec![])).unwrap();
        let dead = heap.alloc(Object::Str("dead".into())).unwrap();
        let minor = heap.collect_young([live]).unwrap();
        assert!(!minor.major);
        assert_eq!(minor.young_reclaimed, 1);
        assert_eq!(minor.promoted, 1);
        assert!(heap.get(live).is_ok());
        assert!(heap.get(dead).is_err());

        let young_dead = heap.alloc(Object::Str("young-dead".into())).unwrap();
        let minor = heap.collect_young([]).unwrap();
        assert_eq!(minor.young_reclaimed, 1);
        assert!(
            heap.get(live).is_ok(),
            "minor must not trace or reclaim old space"
        );
        assert!(heap.get(young_dead).is_err());

        let major = heap.collect([]).unwrap();
        assert!(major.major);
        assert_eq!(major.reclaimed, 1);
        assert!(heap.get(live).is_err());
    }

    #[test]
    fn remembered_set_keeps_old_to_young_mutations_alive() {
        let mut heap = Heap::default();
        let list = heap.alloc(Object::List(vec![Value::NONE])).unwrap();
        let cell = heap.alloc(Object::Cell(Value::NONE)).unwrap();
        let dict = heap
            .alloc(Object::Dict(crate::dict::Dict::default()))
            .unwrap();
        let module = heap.alloc(Object::Module(Vec::new())).unwrap();
        heap.collect_young([list, cell, dict, module]).unwrap();

        let list_value = heap.alloc(Object::Str("list".into())).unwrap();
        let appended_value = heap.alloc(Object::Str("appended".into())).unwrap();
        let cell_value = heap.alloc(Object::Str("cell".into())).unwrap();
        let dict_key = heap.alloc(Object::Str("key".into())).unwrap();
        let dict_value = heap.alloc(Object::Str("value".into())).unwrap();
        let module_value = heap.alloc(Object::Str("module".into())).unwrap();
        heap.set_item(list, Value::int(0).unwrap(), list_value)
            .unwrap();
        heap.append_list(list, appended_value).unwrap();
        heap.store_cell(cell, cell_value).unwrap();
        heap.dict_set(dict, dict_key, dict_value).unwrap();
        heap.add_module_member(module, "member", module_value)
            .unwrap();
        heap.assert_remembered_set_complete();

        let minor = heap.collect_young([list, cell, dict, module]).unwrap();
        assert_eq!(minor.promoted, 6);
        for value in [
            list_value,
            appended_value,
            cell_value,
            dict_key,
            dict_value,
            module_value,
        ] {
            assert!(heap.get(value).is_ok());
        }
        assert_eq!(heap.cell(cell).unwrap(), cell_value);
        assert_eq!(heap.dict_get(dict, dict_key).unwrap(), Some(dict_value));
        assert!(matches!(
            heap.get(list),
            Ok(Object::List(values)) if values == &vec![list_value, appended_value]
        ));
        assert!(matches!(
            heap.get(module),
            Ok(Object::Module(members)) if members == &vec![("member".into(), module_value)]
        ));
    }
    #[test]
    fn deterministic_mutation_graph_matches_reachability_model() {
        let mut heap = Heap::default();
        let mut values = Vec::new();
        let mut edges: Vec<Vec<usize>> = Vec::new();
        let mut alive = Vec::new();
        let mut roots = Vec::new();
        let mut seed = 17u64;
        let mut random = |bound: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as usize % bound
        };
        for _ in 0..200 {
            for _ in 0..16 {
                let id = values.len();
                values.push(heap.alloc(Object::List(vec![])).unwrap());
                edges.push(Vec::new());
                alive.push(true);
                if roots.len() < 8 {
                    roots.push(id);
                } else {
                    let from = roots[random(roots.len())];
                    if random(3) == 0 {
                        heap.append_list(values[from], values[id]).unwrap();
                        edges[from].push(id);
                    }
                    let to = roots[random(roots.len())];
                    if random(3) == 0 {
                        heap.append_list(values[id], values[to]).unwrap();
                        edges[id].push(to);
                    }
                    roots[random(8)] = id;
                }
                heap.append_list(values[id], values[id]).unwrap();
                edges[id].push(id);
            }
            let mut expected = vec![false; values.len()];
            let mut work = roots.clone();
            while let Some(id) = work.pop() {
                if !expected[id] {
                    expected[id] = true;
                    work.extend_from_slice(&edges[id]);
                }
            }
            let stats = heap.collect(roots.iter().map(|id| values[*id])).unwrap();
            assert_eq!(stats.survivors, expected.iter().filter(|v| **v).count());
            for id in 0..values.len() {
                assert_eq!(heap.get(values[id]).is_ok(), expected[id]);
                if expected[id] {
                    assert!(alive[id], "collected object resurrected through slot reuse");
                    let Object::List(items) = heap.get(values[id]).unwrap() else {
                        unreachable!()
                    };
                    assert_eq!(
                        items,
                        &edges[id].iter().map(|i| values[*i]).collect::<Vec<_>>()
                    );
                }
            }
            alive = expected;
        }
        assert_eq!(heap.collect([]).unwrap().survivors, 0);
    }
}
