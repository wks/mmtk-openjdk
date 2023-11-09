use crate::edges::OpenJDKEdge;
use crate::edges::OpenJDKEdgeRange;
use crate::OpenJDK;

use super::abi::*;
use super::UPCALLS;
use mmtk::util::opaque_pointer::*;
use mmtk::util::{Address, ObjectReference};
use mmtk::vm::EdgeVisitor;
use std::cell::UnsafeCell;
use std::mem;

const USE_SLICE_FOR_OOP_MAP_BLOCK: bool = false;

trait OopIterate: Sized {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    );
}

impl OopIterate for OopMapBlock {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        let log_bytes_in_oop = OpenJDKEdge::<COMPRESSED>::LOG_BYTES_IN_EDGE;
        let start = oop.get_field_address(self.offset);
        if USE_SLICE_FOR_OOP_MAP_BLOCK {
            let end = start + ((self.count as usize) << log_bytes_in_oop);
            let slice = OpenJDKEdgeRange::<COMPRESSED>::from(start..end);
            closure.visit_slice(slice);
        } else {
            for i in 0..self.count as usize {
                let edge = (start + (i << log_bytes_in_oop)).into();
                closure.visit_edge(edge);
            }
        }
    }
}

impl OopIterate for InstanceKlass {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        let oop_maps = self.nonstatic_oop_maps();
        for map in oop_maps {
            map.oop_iterate::<COMPRESSED>(oop, closure)
        }
    }
}

impl OopIterate for InstanceMirrorKlass {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        self.instance_klass.oop_iterate::<COMPRESSED>(oop, closure);

        // static fields
        let start = Self::start_of_static_fields(oop);
        let len = Self::static_oop_field_count(oop);
        let end = start + (len << OpenJDKEdge::<COMPRESSED>::LOG_BYTES_IN_EDGE);
        let slice = OpenJDKEdgeRange::<COMPRESSED>::from(start..end);
        closure.visit_slice(slice);
    }
}

impl OopIterate for InstanceClassLoaderKlass {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        self.instance_klass.oop_iterate::<COMPRESSED>(oop, closure);
    }
}

impl OopIterate for ObjArrayKlass {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        let array = unsafe { oop.as_array_oop() };
        let slice = unsafe { array.slice::<COMPRESSED>(BasicType::T_OBJECT) };
        closure.visit_slice(slice)
    }
}

impl OopIterate for TypeArrayKlass {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        _oop: Oop,
        _closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        // Performance tweak: We skip processing the klass pointer since all
        // TypeArrayKlasses are guaranteed processed via the null class loader.
    }
}

impl OopIterate for InstanceRefKlass {
    fn oop_iterate<const COMPRESSED: bool>(
        &self,
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        use crate::abi::*;
        use crate::api::{add_phantom_candidate, add_soft_candidate, add_weak_candidate};
        self.instance_klass.oop_iterate::<COMPRESSED>(oop, closure);

        if Self::should_scan_weak_refs::<COMPRESSED>() {
            let reference = ObjectReference::from(oop);
            match self.instance_klass.reference_type {
                ReferenceType::None => {
                    panic!("oop_iterate on InstanceRefKlass with reference_type as None")
                }
                ReferenceType::Weak => add_weak_candidate(reference),
                ReferenceType::Soft => add_soft_candidate(reference),
                ReferenceType::Phantom => add_phantom_candidate(reference),
                // Process these two types normally (as if they are strong refs)
                // We will handle final reference later
                ReferenceType::Final | ReferenceType::Other => {
                    Self::process_ref_as_strong(oop, closure)
                }
            }
        } else {
            Self::process_ref_as_strong(oop, closure);
        }
    }
}

impl InstanceRefKlass {
    fn should_scan_weak_refs<const COMPRESSED: bool>() -> bool {
        !*crate::singleton::<COMPRESSED>()
            .get_options()
            .no_reference_types
    }
    fn process_ref_as_strong<const COMPRESSED: bool>(
        oop: Oop,
        closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    ) {
        let referent_addr = Self::referent_address::<COMPRESSED>(oop);
        closure.visit_edge(referent_addr);
        let discovered_addr = Self::discovered_address::<COMPRESSED>(oop);
        closure.visit_edge(discovered_addr);
    }
}

#[allow(unused)]
fn oop_iterate_slow<const COMPRESSED: bool, V: EdgeVisitor<OpenJDK<COMPRESSED>>>(
    oop: Oop,
    closure: &mut V,
    tls: OpaquePointer,
) {
    unsafe {
        CLOSURE.with(|x| *x.get() = closure as *mut V as *mut u8);
        ((*UPCALLS).scan_object)(
            mem::transmute(
                scan_object_fn::<COMPRESSED, V> as *const unsafe extern "C" fn(edge: Address),
            ),
            mem::transmute(oop),
            tls,
        );
    }
}

fn oop_iterate<const COMPRESSED: bool>(
    oop: Oop,
    closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
) {
    let klass = oop.klass::<COMPRESSED>();
    let klass_id = klass.id;
    assert!(
        klass_id as i32 >= 0 && (klass_id as i32) < 6,
        "Invalid klass-id: {:x} for oop: {:x}",
        klass_id as i32,
        unsafe { mem::transmute::<Oop, ObjectReference>(oop) }
    );
    match klass_id {
        KlassID::Instance => {
            let instance_klass = unsafe { klass.cast::<InstanceKlass>() };
            instance_klass.oop_iterate::<COMPRESSED>(oop, closure);
        }
        KlassID::InstanceClassLoader => {
            let instance_klass = unsafe { klass.cast::<InstanceClassLoaderKlass>() };
            instance_klass.oop_iterate::<COMPRESSED>(oop, closure);
        }
        KlassID::InstanceMirror => {
            let instance_klass = unsafe { klass.cast::<InstanceMirrorKlass>() };
            instance_klass.oop_iterate::<COMPRESSED>(oop, closure);
        }
        KlassID::ObjArray => {
            let array_klass = unsafe { klass.cast::<ObjArrayKlass>() };
            array_klass.oop_iterate::<COMPRESSED>(oop, closure);
        }
        KlassID::TypeArray => {
            // Skip scanning primitive arrays as they contain no reference fields.
        }
        KlassID::InstanceRef => {
            let instance_klass = unsafe { klass.cast::<InstanceRefKlass>() };
            instance_klass.oop_iterate::<COMPRESSED>(oop, closure);
        }
    }
}

thread_local! {
    static CLOSURE: UnsafeCell<*mut u8> = UnsafeCell::new(std::ptr::null_mut());
}

pub unsafe extern "C" fn scan_object_fn<
    const COMPRESSED: bool,
    V: EdgeVisitor<OpenJDK<COMPRESSED>>,
>(
    edge: Address,
) {
    let ptr: *mut u8 = CLOSURE.with(|x| *x.get());
    let closure = &mut *(ptr as *mut V);
    closure.visit_edge(edge.into());
}

pub fn scan_object<const COMPRESSED: bool>(
    object: ObjectReference,
    closure: &mut impl EdgeVisitor<OpenJDK<COMPRESSED>>,
    _tls: VMWorkerThread,
) {
    unsafe { oop_iterate::<COMPRESSED>(mem::transmute(object), closure) }
}
