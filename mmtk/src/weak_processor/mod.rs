use std::sync::Mutex;

use log::debug;
use mmtk::vm::ProcessWeakRefsContext;

use self::{finalizable_processor::FinalizableProcessor, reference_processor::ReferenceProcessors};

pub(crate) mod finalizable_processor;
pub(crate) mod reference_processor;

#[derive(Debug)]
enum Phase {
    Inactive,
    Soft,
    Weak,
    Final,
    Phantom,
}

pub struct WeakProcessor {
    phase: Phase,
    pub reference_processors: ReferenceProcessors,
    pub finalizable_processor: Mutex<FinalizableProcessor>,
}

impl Default for WeakProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl WeakProcessor {
    pub fn new() -> Self {
        Self {
            phase: Phase::Inactive,
            reference_processors: ReferenceProcessors::new(),
            finalizable_processor: Mutex::new(FinalizableProcessor::new()),
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.phase, Phase::Inactive)
    }

    pub fn process_weak_refs(
        &mut self,
        mut context: impl ProcessWeakRefsContext,
        forwarding: bool,
        nursery: bool,
    ) -> bool {
        if forwarding {
            unimplemented!("Forwarding is not implemented.")
        }

        log::trace!(
            "Entering process_weak_refs. forwarding: {}, nursery: {}",
            forwarding,
            nursery
        );

        'retry_loop: loop {
            log::trace!("Phase: {:?}", self.phase);
            match self.phase {
                Phase::Inactive => {
                    self.phase = Phase::Soft;
                    continue 'retry_loop;
                }
                Phase::Soft => {
                    self.reference_processors
                        .scan_soft_refs(|o| context.trace_object(o));
                    self.phase = Phase::Weak;
                    break 'retry_loop true;
                }
                Phase::Weak => {
                    self.reference_processors
                        .scan_weak_refs(|o| context.trace_object(o));
                    self.phase = Phase::Final;
                    break 'retry_loop true;
                }
                Phase::Final => {
                    let mut finalizable_processor = self.finalizable_processor.lock().unwrap();
                    debug!(
                        "Finalization, {} objects in candidates, {} objects ready to finalize",
                        finalizable_processor.candidates.len(),
                        finalizable_processor.ready_for_finalize.len()
                    );

                    finalizable_processor.scan(|o| context.trace_object(o), nursery);
                    debug!(
                        "Finished finalization, {} objects in candidates, {} objects ready to finalize",
                        finalizable_processor.candidates.len(),
                        finalizable_processor.ready_for_finalize.len()
                    );

                    self.phase = Phase::Phantom;
                    break 'retry_loop true;
                }
                Phase::Phantom => {
                    self.reference_processors
                        .scan_phantom_refs(|o| context.trace_object(o));
                    self.phase = Phase::Inactive;
                    break 'retry_loop false;
                }
            }
        }
    }
}
