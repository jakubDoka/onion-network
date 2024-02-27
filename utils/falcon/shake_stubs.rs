mod shake {
    use {
        crate::src::codec::{size_t, uint8_t},
        core::{mem::ManuallyDrop, ops::DerefMut},
        sha3::digest::{ExtendableOutput, Update, XofReader},
    };

    #[repr(C)]
    pub union Ctx {
        acc: ManuallyDrop<sha3::Shake256>,
        rel: ManuallyDrop<sha3::Shake256Reader>,
        pub uninit: (),
    }

    #[no_mangle]
    pub unsafe extern "C" fn shake256_inc_squeeze(
        output: *mut uint8_t,
        outlen: size_t,
        state: *mut Ctx,
    ) {
        let state = &mut (*state).rel;
        let slice = core::slice::from_raw_parts_mut(output, outlen as usize);
        state.deref_mut().read(slice);
    }

    #[no_mangle]
    pub unsafe extern "C" fn shake256_inc_absorb(
        state: *mut Ctx,
        input: *const uint8_t,
        inlen: size_t,
    ) {
        let state = &mut (*state).acc;
        let slice = core::slice::from_raw_parts(input, inlen as usize);
        state.deref_mut().update(slice);
    }

    #[no_mangle]
    pub unsafe extern "C" fn shake256_inc_init(state: *mut Ctx) {
        let state = core::ptr::addr_of_mut!((*state).acc);
        core::ptr::write(state, ManuallyDrop::new(sha3::Shake256::default()));
    }

    #[no_mangle]
    pub unsafe extern "C" fn shake256_inc_finalize(state: *mut Ctx) {
        let old_state = core::ptr::read(&(*state).acc);
        let new_state = ManuallyDrop::into_inner(old_state).finalize_xof();
        let state = core::ptr::addr_of_mut!((*state).rel);
        core::ptr::write(state, ManuallyDrop::new(new_state));
    }

    #[no_mangle]
    pub unsafe extern "C" fn shake256_inc_ctx_release(state: *mut Ctx) {
        let state = core::ptr::addr_of_mut!((*state).rel);
        core::ptr::drop_in_place(state);
    }
}
