use mmtk::vm::ProcessWeakRefsContext;

use crate::SINGLETON;

use self::reference_processor::ReferenceProcessors;

pub(crate) mod reference_processor;
pub(crate) mod finalizable_processor;

enum Phase {
    Soft,
    Weak,
    Final,
    Phantom,
}

pub struct WeakProcessor {
    phase: Phase,
    reference_processors: ReferenceProcessors
}

impl WeakProcessor {
    pub fn new() -> Self {
        Self {
            phase: Phase::Soft,
            reference_processors: ReferenceProcessors::new(),
        }
    }

    pub fn process_weak_refs(
        &mut self,
        mut context: impl ProcessWeakRefsContext,
        forwarding: bool,
        nursery: bool,
    ) -> bool {
        'retry_loop: loop {
            match self.phase {
                Phase::Soft => {
                    self.reference_processors.scan_soft_refs(|o| context.trace_object(o), &SINGLETON);
                    self.phase = Phase::Weak;
                    break 'retry_loop true
                }
                Phase::Weak => {
                    self.reference_processors.scan_weak_refs(|o| context.trace_object(o), &SINGLETON);
                    self.phase = Phase::Final;
                    break 'retry_loop true
                },
                Phase::Final => {
                    self.phase = Phase::Phantom;
                    break 'retry_loop true
                },
                Phase::Phantom => {
                    self.reference_processors.scan_weak_refs(|o| context.trace_object(o), &SINGLETON);
                    break 'retry_loop false
                },
            }
        }
    }
}
