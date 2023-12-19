use libc::*;
use super::*;
pub use super::common::*;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ibv_gid_global_t {
    pub subnet_prefix: u64,
    pub interface_id: u64,
}

#[repr(C)]
pub union ibv_async_event_element_union_t {
    pub cq: *mut ibv_cq,
    pub qp: *mut ibv_qp,
    pub srq: *mut ibv_srq,
    pub dct: *mut ibv_exp_dct,
    pub port_num: c_int,
    pub xrc_qp_num: u32,
}

#[repr(C)]
pub struct ibv_wc {
    pub wr_id: u64,
    pub status: ibv_wc_status::Type,
    pub opcode: ibv_wc_opcode::Type,
    pub vendor_err: u32,
    pub byte_len: u32,
    pub imm_data: u32,
    pub qp_num: u32,
    pub src_qp: u32,
    pub wc_flags: c_uint,
    pub pkey_index: u16,
    pub slid: u16,
    pub sl: u8,
    pub dlid_path_bits: u8,
}

impl ibv_wc {
    /// Get the immediate data.
    #[inline(always)]
    pub fn imm(&self) -> u32 {
        self.imm_data
    }
}

#[inline]
pub unsafe fn ___ibv_query_port(
    context: *mut ibv_context,
    port_num: u8,
    port_attr: *mut ibv_port_attr,
) -> ::std::os::raw::c_int {
    (*port_attr).link_layer = IBV_LINK_LAYER_UNSPECIFIED as u8;
    (*port_attr).reserved = 0;

    ibv_query_port(context, port_num, port_attr)
}

#[inline]
pub(super) unsafe fn verbs_get_ctx(ctx: *const ibv_context) -> *mut verbs_context {
    const __VERBS_ABI_IS_EXTENDED: *mut ::std::os::raw::c_void =
        std::ptr::null_mut::<u8>().wrapping_sub(1) as _;
    if ctx.is_null() || (*ctx).abi_compat != __VERBS_ABI_IS_EXTENDED {
        std::ptr::null_mut()
    } else {
        container_of!(ctx, verbs_context, context)
    }
}

#[inline]
pub unsafe fn ibv_create_flow(qp: *mut ibv_qp, flow_attr: *mut ibv_flow_attr) -> *mut ibv_flow {
    let vctx = verbs_get_ctx_op!((*qp).context, create_flow);
    if vctx.is_null() {
        std::ptr::null_mut()
    } else {
        (*vctx).create_flow.unwrap()(qp, flow_attr)
    }
}

#[inline]
pub unsafe fn ibv_destroy_flow(flow_id: *mut ibv_flow) -> ::std::os::raw::c_int {
    let vctx = verbs_get_ctx_op!((*flow_id).context, destroy_flow);
    if vctx.is_null() {
        -ENOSYS
    } else {
        (*vctx).destroy_flow.unwrap()(flow_id)
    }
}

/// Open an extended connection domain.
#[inline]
pub unsafe fn ibv_open_xrcd(
    context: *mut ibv_context,
    xrcd_init_attr: *mut ibv_xrcd_init_attr,
) -> *mut ibv_xrcd {
    let vctx = verbs_get_ctx_op!(context, open_xrcd);
    if vctx.is_null() {
        *__errno_location() = ENOSYS;
        std::ptr::null_mut()
    } else {
        (*vctx).open_xrcd.unwrap()(context, xrcd_init_attr)
    }
}

/// Allocate a memory window.
#[inline]
pub unsafe fn ibv_alloc_mw(pd: *mut ibv_pd, type_: ibv_mw_type::Type) -> *mut ibv_mw {
    if let Some(alloc_mw) = (*(*pd).context).ops.alloc_mw {
        alloc_mw(pd, type_)
    } else {
        *__errno_location() = ENOSYS;
        std::ptr::null_mut()
    }
}

#[inline]
unsafe fn verbs_get_exp_ctx(ctx: *const ibv_context) -> *mut verbs_context_exp {
    let app_ex_ctx = verbs_get_ctx(ctx);
    if app_ex_ctx.is_null()
        || (*app_ex_ctx).has_comp_mask & verbs_context_mask::VERBS_CONTEXT_EXP.0 == 0
    {
        std::ptr::null_mut()
    } else {
        let actual_ex_ctx =
            ((ctx as usize) - ((*app_ex_ctx).sz - std::mem::size_of::<ibv_context>())) as *mut u8;
        (actual_ex_ctx as usize - std::mem::size_of::<verbs_context_exp>()) as *mut _
    }
}

macro_rules! IBV_EXP_RET_ON_INVALID_COMP_MASK_compat {
    ($val:expr, $valid_mask:expr, $ret:expr, $func:expr) => {{
        if (($val) > ($valid_mask)) {
            let __val: ::std::os::raw::c_ulonglong = ($val) as _;
            let __valid_mask: ::std::os::raw::c_ulonglong = ($valid_mask) as _;

            // NOTE: since we cannot easily acquire `stderr: *mut FILE`, we use `eprintln!` instead.
            // Compatibility issues may occur, but since this is debug info it should be fine.
            eprintln!(
                "{}: invalid comp_mask !!! (comp_mask = 0x{:x} valid_mask = 0x{:x})\n",
                $func, __val, __valid_mask,
            );
            *(::libc::__errno_location()) = ::libc::EINVAL;
            return $ret;
        }
    }};
}

#[allow(unused)]
macro_rules! IBV_EXP_RET_NULL_ON_INVALID_COMP_MASK_compat {
    ($val:expr, $valid_mask:expr, $func:expr) => {
        IBV_EXP_RET_ON_INVALID_COMP_MASK_compat!($val, $valid_mask, ::std::ptr::null_mut(), $func,)
    };
}

#[allow(unused)]
macro_rules! IBV_EXP_RET_EINVAL_ON_INVALID_COMP_MASK_compat {
    ($val:expr, $valid_mask:expr, $func:expr) => {
        IBV_EXP_RET_ON_INVALID_COMP_MASK_compat!($val, $valid_mask, ::libc::EINVAL, $func)
    };
}

#[allow(unused)]
macro_rules! IBV_EXP_RET_ZERO_ON_INVALID_COMP_MASK_compat {
    ($val:expr, $valid_mask:expr, $func:expr) => {
        IBV_EXP_RET_ON_INVALID_COMP_MASK_compat!($val, $valid_mask, 0, $func)
    };
}

macro_rules! verbs_get_exp_ctx_op {
    ($ctx:expr, $op:ident) => {{
        let vctx = verbs_get_exp_ctx($ctx);
        if vctx.is_null()
            || (*vctx).sz
                < ::std::mem::size_of_val(&*vctx) - memoffset::offset_of!(verbs_context_exp, $op)
            || (*vctx).$op.is_none()
        {
            std::ptr::null_mut()
        } else {
            vctx
        }
    }};
}

/// Query GID attributes.
#[inline]
pub unsafe fn ibv_exp_query_gid_attr(
    context: *mut ibv_context,
    port_num: u8,
    index: ::std::os::raw::c_uint,
    attr: *mut ibv_exp_gid_attr,
) -> ::std::os::raw::c_int {
    let vctx = verbs_get_exp_ctx_op!(context, exp_query_gid_attr);
    if vctx.is_null() {
        ENOSYS
    } else {
        IBV_EXP_RET_EINVAL_ON_INVALID_COMP_MASK_compat!(
            (*attr).comp_mask,
            IBV_EXP_QUERY_GID_ATTR_RESERVED - 1,
            "ibv_exp_query_gid_attr"
        );
        (*vctx).exp_query_gid_attr.unwrap()(context, port_num, index, attr)
    }
}