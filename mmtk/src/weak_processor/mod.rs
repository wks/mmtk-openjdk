use std::sync::Mutex;

use log::debug;
use mmtk::{
    scheduler::{GCWork, GCWorker, WorkBucketStage},
    vm::{ObjectTracer, ProcessWeakRefsContext, QueuingTracerFactory},
};

use crate::{OpenJDK, WEAK_PROCESSOR};

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
        worker: &mut GCWorker<OpenJDK>,
        context: ProcessWeakRefsContext,
        tracer_factory: impl QueuingTracerFactory<OpenJDK>,
    ) -> bool {
        let forwarding = context.forwarding;
        let nursery = context.nursery;

        if forwarding {
            assert!(matches!(self.phase, Phase::Inactive));
            tracer_factory.with_queuing_tracer(worker, |tracer| {
                self.reference_processors
                    .forward_refs(|o| tracer.trace_object(o));
                {
                    let mut finalizable_processor = self.finalizable_processor.lock().unwrap();
                    finalizable_processor
                        .forward_candidate(&mut |o| tracer.trace_object(o), nursery);
                    finalizable_processor
                        .forward_finalizable(&mut |o| tracer.trace_object(o), nursery);
                }
            });
            return false;
        }

        log::trace!(
            "Entering process_weak_refs. forwarding: {}, nursery: {}",
            forwarding,
            nursery,
        );

        'retry_loop: loop {
            log::trace!("Phase: {:?}", self.phase);
            match self.phase {
                Phase::Inactive => {
                    self.phase = Phase::Soft;
                    continue 'retry_loop;
                }
                Phase::Soft => {
                    tracer_factory.with_queuing_tracer(worker, |tracer| {
                        self.reference_processors
                            .scan_soft_refs(|o| tracer.trace_object(o));
                    });
                    self.phase = Phase::Weak;
                    break 'retry_loop true;
                }
                Phase::Weak => {
                    // This is not necessary.
                    // I am testing if the QueuingTracerFactory can be
                    // cloned and sent to another work packet.
                    let work = ProcessWeakRefsWork {
                        tracer_factory: tracer_factory.clone(),
                    };
                    worker.scheduler().work_buckets[WorkBucketStage::VMRefClosure].add(work);
                    self.phase = Phase::Final;
                    break 'retry_loop true;
                }
                Phase::Final => {
                    tracer_factory.with_queuing_tracer(worker, |tracer| {

                        let mut finalizable_processor = self.finalizable_processor.lock().unwrap();
                        debug!(
                            "Finalization, {} objects in candidates, {} objects ready to finalize",
                            finalizable_processor.candidates.len(),
                            finalizable_processor.ready_for_finalize.len()
                        );

                        finalizable_processor.scan(|o| tracer.trace_object(o), nursery);
                        debug!(
                            "Finished finalization, {} objects in candidates, {} objects ready to finalize",
                            finalizable_processor.candidates.len(),
                            finalizable_processor.ready_for_finalize.len()
                        );
                    });

                    self.phase = Phase::Phantom;
                    break 'retry_loop true;
                }
                Phase::Phantom => {
                    tracer_factory.with_queuing_tracer(worker, |tracer| {
                        self.reference_processors
                            .scan_phantom_refs(|o| tracer.trace_object(o));
                    });
                    self.phase = Phase::Inactive;
                    break 'retry_loop false;
                }
            }
        }
    }
}

struct ProcessWeakRefsWork<T: QueuingTracerFactory<OpenJDK>> {
    tracer_factory: T,
}

impl<T: QueuingTracerFactory<OpenJDK>> GCWork<OpenJDK> for ProcessWeakRefsWork<T> {
    fn do_work(&mut self, worker: &mut GCWorker<OpenJDK>, _mmtk: &'static mmtk::MMTK<OpenJDK>) {
        self.tracer_factory.with_queuing_tracer(worker, |tracer| {
            let weak_processor = loop {
                if let Ok(wp) = WEAK_PROCESSOR.try_borrow_mut() {
                    break wp;
                }
            };
            weak_processor
                .reference_processors
                .scan_weak_refs(|o| tracer.trace_object(o));
        });
    }
}
