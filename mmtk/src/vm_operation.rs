use crate::UPCALLS;

struct RustClosure<'a> {
    func: &'a mut dyn FnMut(),
}

/// Run a function in the OpenJDK VM Thread.
/// 
/// This function is supposed to be called in Rust.
pub fn run_in_vm_thread<T>(evaluate_in_safepiont: bool, mut f: T) where T: FnMut() {
    let mut rust_closure = RustClosure {
        func: &mut f,
    };
        
    unsafe {
        ((*UPCALLS).run_in_vm_thread)(&mut rust_closure as *mut RustClosure as *mut ::libc::c_void,
            evaluate_in_safepiont);
    }
}

/// Run a rust closure from C++.
///
/// This function is supposed to be called by C++ in `VM_MMTkRustOperation`.
#[no_mangle]
pub unsafe extern "C" fn mmtk_run_rust_vm_operation(rust_closure: *mut ::libc::c_void) {
    let rust_closure: &mut RustClosure = &mut *(rust_closure as *mut RustClosure);
    (rust_closure.func)();
}