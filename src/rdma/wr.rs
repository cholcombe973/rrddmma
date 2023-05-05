use std::marker::PhantomData;
use std::{mem, ptr};

use rdma_sys::*;

use super::mr::*;
use super::qp::{build_sgl, QpPeer};

/// Wrapper of basic parameters of a RDMA work request.
///
/// Every work request must at least contain:
/// - a list of registered memory areas (can be empty) as the data resource
///   or target,
/// - a work request ID, and
/// - a set of flags (currently, only to signal or not).
pub struct WrBase<'a> {
    local: Vec<ibv_sge>,
    wr_id: u64,
    signal: bool,

    /// Pretend to hold a reference to the original memory regions even if we
    /// have already transformed the slices into a scatter-gather list.
    /// This prevents the SGL from being invalid.
    marker: PhantomData<&'a Mr<'a>>,
}

/// Send work request elements other than the basics.
pub enum SendWrDetails<'a> {
    /// Send requires basic parameters and an optional immediate.
    Send(Option<u32>),

    /// Send via UD QPs requires specifying the target and an optional immediate.
    SendTo(QpPeer, Option<u32>),

    /// Read requires a remote memory area to read from.
    Read(&'a RemoteMrSlice<'a>),

    /// Write requires a remote memory area to write to and an optional immediate.
    Write(&'a RemoteMrSlice<'a>, Option<u32>),
}

/// Send work request.
///
/// Use this type when you want to post multiple send work requests to a
/// queue pair at once (which can reduce doorbell ringing overheads).
pub struct SendWr<'a>(WrBase<'a>, SendWrDetails<'a>);

impl<'a> SendWr<'a> {
    pub fn new(
        local: &'a [MrSlice<'a>],
        wr_id: u64,
        signal: bool,
        additions: SendWrDetails<'a>,
    ) -> Self {
        Self(
            WrBase {
                local: build_sgl(local),
                wr_id,
                signal,
                marker: PhantomData,
            },
            additions,
        )
    }

    /// Translate the `SendWr` into a `ibv_send_wr` that can be passed to
    /// `ibv_post_send`.
    ///
    /// # Safety
    ///
    /// The resulted `ibv_send_wr` is a foreign type and has no connection with
    /// the original `SendWr`, which holds the scatter-gather list. The compiler
    /// therefore cannot enforce that the `RecvWr` outlives the return value.
    ///
    /// The caller must ensure that the `SendWr` is not dropped before the
    /// return value is posted to the RDMA device.
    pub unsafe fn to_wr(&self) -> ibv_send_wr {
        let mut wr = unsafe { mem::zeroed::<ibv_send_wr>() };

        wr.wr_id = self.0.wr_id;
        wr.sg_list = self.0.local.as_ptr() as *mut _;
        wr.num_sge = self.0.local.len() as i32;
        wr.send_flags = if self.0.signal {
            ibv_send_flags::IBV_SEND_SIGNALED.0
        } else {
            0
        };
        wr.next = ptr::null_mut();

        fn fill_opcode_with_imm(
            wr: &mut ibv_send_wr,
            imm: &Option<u32>,
            op: ibv_wr_opcode::Type,
            op_with_imm: ibv_wr_opcode::Type,
        ) {
            if let Some(imm) = imm {
                wr.opcode = op_with_imm;
                wr.imm_data_invalidated_rkey_union =
                    imm_data_invalidated_rkey_union_t { imm_data: *imm };
            } else {
                wr.opcode = op;
            }
        }
        match &self.1 {
            SendWrDetails::Send(imm) => fill_opcode_with_imm(
                &mut wr,
                &imm,
                ibv_wr_opcode::IBV_WR_SEND,
                ibv_wr_opcode::IBV_WR_SEND_WITH_IMM,
            ),
            SendWrDetails::SendTo(peer, imm) => {
                wr.wr.ud = ud_t::from(peer);
                fill_opcode_with_imm(
                    &mut wr,
                    &imm,
                    ibv_wr_opcode::IBV_WR_SEND,
                    ibv_wr_opcode::IBV_WR_SEND_WITH_IMM,
                );
            }
            SendWrDetails::Read(remote) => {
                wr.wr.rdma = rdma_t::from(*remote);
                wr.opcode = ibv_wr_opcode::IBV_WR_RDMA_READ;
            }
            SendWrDetails::Write(remote, imm) => {
                wr.wr.rdma = rdma_t::from(*remote);
                fill_opcode_with_imm(
                    &mut wr,
                    &imm,
                    ibv_wr_opcode::IBV_WR_RDMA_WRITE,
                    ibv_wr_opcode::IBV_WR_RDMA_WRITE_WITH_IMM,
                );
            }
        };

        wr
    }
}

/// Receive work request.
///
/// Equivalent to the work request basics.
///
/// Use this type when you want to post multiple recv work requests to a
/// queue pair at once (which can reduce doorbell ringing overheads).
pub struct RecvWr<'a>(WrBase<'a>);

impl<'a> RecvWr<'a> {
    pub fn new(local: &'a [MrSlice<'a>], wr_id: u64, signal: bool) -> Self {
        Self(WrBase {
            local: build_sgl(local),
            wr_id,
            signal,
            marker: PhantomData,
        })
    }

    /// Translate the `RecvWr` into a `ibv_recv_wr` that can be passed to
    /// `ibv_post_recv`.
    ///
    /// # Safety
    ///
    /// The resulted `ibv_recv_wr` is a foreign type and has no connection with
    /// the original `RecvWr`, which holds the scatter-gather list. The compiler
    /// therefore cannot enforce that the `RecvWr` outlives the return value.
    ///
    /// The caller must ensure that the `RecvWr` is not dropped before the
    /// return value is posted to the RDMA device.
    pub unsafe fn to_wr(&self) -> ibv_recv_wr {
        ibv_recv_wr {
            wr_id: self.0.wr_id,
            sg_list: self.0.local.as_ptr() as *mut _,
            num_sge: self.0.local.len() as i32,
            next: ptr::null_mut(),
        }
    }
}
