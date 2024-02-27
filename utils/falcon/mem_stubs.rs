mod mem {
    use crate::libc;

    #[no_mangle]
    pub unsafe extern "C" fn rust_memmove(
        dst: *mut libc::c_void,
        src: *const libc::c_void,
        n: libc::c_ulong,
    ) -> *mut libc::c_void {
        core::ptr::copy(src as *const u8, dst as *mut u8, n as usize);
        core::ptr::null_mut()
    }

    #[no_mangle]
    pub unsafe extern "C" fn rust_memcpy(
        dst: *mut libc::c_void,
        src: *const libc::c_void,
        n: libc::c_ulong,
    ) -> *mut libc::c_void {
        core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, n as usize);
        core::ptr::null_mut()
    }

    #[no_mangle]
    pub unsafe extern "C" fn rust_memset(
        dst: *mut libc::c_void,
        c: libc::c_int,
        n: libc::c_ulong,
    ) -> *mut libc::c_void {
        core::ptr::write_bytes(dst as *mut u8, c as u8, n as usize);
        core::ptr::null_mut()
    }
}
