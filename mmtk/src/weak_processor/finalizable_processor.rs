use log::trace;
use mmtk::{util::ObjectReference, vm::Finalizable};

use crate::collection::VMCollection;

/// A special processor for Finalizable objects.
// TODO: we should consider if we want to merge FinalizableProcessor with ReferenceProcessor,
// and treat final reference as a special reference type in ReferenceProcessor.
#[derive(Default)]
pub struct FinalizableProcessor {
    /// Candidate objects that has finalizers with them
    pub candidates: Vec<ObjectReference>,
    /// Index into candidates to record where we are up to in the last scan of the candidates.
    /// Index after nursery_index are new objects inserted after the last GC.
    pub nursery_index: usize,
    /// Objects that can be finalized. They are actually dead, but we keep them alive
    /// until the binding pops them from the queue.
    pub ready_for_finalize: Vec<ObjectReference>,
}

impl FinalizableProcessor {
    pub fn new() -> Self {
        Self {
            candidates: vec![],
            nursery_index: 0,
            ready_for_finalize: vec![],
        }
    }

    pub fn add(&mut self, object: ObjectReference) {
        self.candidates.push(object);
    }

    fn forward_finalizable_reference<E>(trace_object: &mut E, finalizable: &mut ObjectReference)
    where
        E: FnMut(ObjectReference) -> ObjectReference,
    {
        *finalizable = trace_object(*finalizable);
    }

    pub fn scan<E>(&mut self, mut trace_object: E, nursery: bool)
    where
        E: FnMut(ObjectReference) -> ObjectReference,
    {
        let start = if nursery { self.nursery_index } else { 0 };

        // We should go through ready_for_finalize objects and keep them alive.
        // Unlike candidates, those objects are known to be alive. This means
        // theoratically we could do the following loop at any time in a GC (not necessarily after closure phase).
        // But we have to iterate through candidates after closure.
        self.candidates.append(&mut self.ready_for_finalize);
        debug_assert!(self.ready_for_finalize.is_empty());

        for mut f in self
            .candidates
            .drain(start..)
            .collect::<Vec<ObjectReference>>()
        {
            let reff = f.get_reference();
            trace!("Pop {:?} for finalization", reff);
            if reff.is_live() {
                Self::forward_finalizable_reference(&mut trace_object, &mut f);
                trace!("{:?} is live, push {:?} back to candidates", reff, f);
                self.candidates.push(f);
                continue;
            }

            // We should not at this point mark the object as live. A binding may register an object
            // multiple times with different finalizer methods. If we mark the object as live here, and encounter
            // the same object later in the candidates list (possibly with a different finalizer method),
            // we will erroneously think the object never died, and won't push it to the ready_to_finalize
            // queue.
            // So we simply push the object to the ready_for_finalize queue, and mark them as live objects later.
            self.ready_for_finalize.push(f);
        }

        // Keep the finalizable objects alive.
        self.forward_finalizable(&mut trace_object, nursery);

        self.nursery_index = self.candidates.len();

        VMCollection::schedule_finalization2();
    }

    pub fn forward_candidate<E>(&mut self, trace_object: &mut E, _nursery: bool)
    where
        E: FnMut(ObjectReference) -> ObjectReference,
    {
        self.candidates
            .iter_mut()
            .for_each(|f| Self::forward_finalizable_reference(trace_object, f));
    }

    pub fn forward_finalizable<E>(&mut self, trace_object: &mut E, _nursery: bool)
    where
        E: FnMut(ObjectReference) -> ObjectReference,
    {
        self.ready_for_finalize
            .iter_mut()
            .for_each(|f| Self::forward_finalizable_reference(trace_object, f));
    }

    pub fn get_ready_object(&mut self) -> Option<ObjectReference> {
        self.ready_for_finalize.pop()
    }

    pub fn get_all_finalizers(&mut self) -> Vec<ObjectReference> {
        let mut ret = std::mem::take(&mut self.candidates);
        let ready_objects = std::mem::take(&mut self.ready_for_finalize);

        ret.extend(ready_objects);
        ret
    }

    pub fn get_finalizers_for(&mut self, object: ObjectReference) -> Vec<ObjectReference> {
        // Drain filter for finalizers that equal to 'object':
        // * for elements that equal to 'object', they will be removed from the original vec, and returned.
        // * for elements that do not equal to 'object', they will be left in the original vec.
        // TODO: We should replace this with `vec.drain_filter()` when it is stablized.
        let drain_filter = |vec: &mut Vec<ObjectReference>| -> Vec<ObjectReference> {
            let mut i = 0;
            let mut ret = vec![];
            while i < vec.len() {
                if vec[i].get_reference() == object {
                    let val = vec.remove(i);
                    ret.push(val);
                } else {
                    i += 1;
                }
            }
            ret
        };
        let mut ret: Vec<ObjectReference> = drain_filter(&mut self.candidates);
        ret.extend(drain_filter(&mut self.ready_for_finalize));
        ret
    }
}
