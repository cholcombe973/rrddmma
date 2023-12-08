use std::ptr::NonNull;
use std::sync::Arc;
use std::{fmt, io, mem};

use super::cq::Cq;
use super::device::*;
use super::gid::Gid;
use super::pd::Pd;
use crate::utils::interop::*;

use crate::bindings::*;
use anyhow::{Context as _, Result};

#[allow(dead_code)]
struct ContextInner {
    ctx: NonNull<ibv_context>,
    dev_attr: ibv_device_attr,

    port_attr: ibv_port_attr,
    port_num: u8,
    gid: Gid,
    gid_index: u8,
}

unsafe impl Send for ContextInner {}
unsafe impl Sync for ContextInner {}

impl fmt::Debug for ContextInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("ctx", &self.ctx)
            .field("gid", &self.gid)
            .finish()
    }
}

impl Drop for ContextInner {
    fn drop(&mut self) {
        // SAFETY: FFI.
        unsafe { ibv_close_device(self.ctx.as_ptr()) };
    }
}

/// Device context.
///
/// This type is a simple wrapper of an `Arc` and is guaranteed to have the
/// same memory layout with it.
///
/// Rather than a pure `ibv_context`, you also need to specify a device port
/// when creating an instance of this type. To operate on different ports of
/// the same device, it is required to create multiple `Context` instances.
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct Context {
    inner: Arc<ContextInner>,
}

impl Context {
    /// Open a device and query the related attributes (device and port).
    ///
    /// # Device and port selection
    ///
    /// Behavior depends on whether you specify a device name:
    /// - If `dev_name` is `Some`, the device with the given name will be used,
    ///   and `port_num` represents the port number of the device. An error will
    ///   occur if the device does not exist or the port is not active.
    /// - If `dev_name` is `None`, all devices will be iterated and the
    ///   `port_num`-th active port will be used. Port numbers start from 1.
    ///   An error will occur if there are not enough active ports.
    ///   This behavior is greatly inspired by [eRPC](https://github.com/erpc-io/eRPC/blob/094c17c3cd9b48bcfbed63f455cc85b9976bd43f/src/transport_impl/verbs_common.h#L129).
    pub fn open(dev_name: Option<&str>, port_num: u8, gid_index: u8) -> Result<Self> {
        if port_num == 0 {
            return Err(anyhow::anyhow!("port number must be non-zero"));
        }
        let dev_list = DeviceList::new()?;
        let (ctx, dev_attr, port_num, port_attr) = if let Some(dev_name) = dev_name {
            // SAFETY: No other references to the device list here.
            let dev = unsafe { dev_list.as_slice() }
                .iter()
                .find(|dev| dev_name == dev.name())
                .ok_or_else(|| anyhow::anyhow!("device not found"))?;

            // SAFETY: FFI.
            let ctx = NonNull::new(unsafe { ibv_open_device(dev.as_raw()) })
                .ok_or_else(|| anyhow::anyhow!(io::Error::last_os_error()))?;
            drop(dev_list);

            let dev_attr = query_device(ctx.as_ptr())?;
            if port_num > dev_attr.phys_port_cnt {
                return Err(anyhow::anyhow!("invalid port number {}", port_num));
            }

            let port_attr = query_port(ctx.as_ptr(), port_num)?;
            if port_attr.state != ibv_port_state::IBV_PORT_ACTIVE {
                return Err(anyhow::anyhow!("port {} is not active", port_num));
            }
            (ctx, dev_attr, port_num, port_attr)
        } else {
            let DevicePort(ctx, port_num) = dev_list
                .iter()
                .nth(port_num as usize - 1)
                .ok_or_else(|| anyhow::anyhow!("not enough active ports found"))??;
            let dev_attr = query_device(ctx)?;
            let port_attr = query_port(ctx, port_num)?;

            // SAFETY: `ibv_context` pointer returned by `DeviceList::iter()` is
            // guaranteed to be non-null.
            let ctx = unsafe { NonNull::new_unchecked(ctx) };
            (ctx, dev_attr, port_num, port_attr)
        };

        let gid = {
            let gid_index = (gid_index as i32 % port_attr.gid_tbl_len) as u8;
            // SAFETY: will be filled by the FFI call.
            let mut gid = unsafe { mem::zeroed() };
            // SAFETY: FFI.
            let ret = unsafe { ibv_query_gid(ctx.as_ptr(), port_num, gid_index as i32, &mut gid) };
            if ret != 0 {
                return from_c_err(ret).with_context(|| "failed to query GID");
            }
            Gid::from(gid)
        };

        Ok(Context {
            inner: Arc::new(ContextInner {
                ctx,
                dev_attr,
                port_attr,
                port_num,
                gid,
                gid_index,
            }),
        })
    }

    /// Get the underlying `ibv_context` pointer.
    #[inline]
    pub fn as_raw(&self) -> *mut ibv_context {
        self.inner.ctx.as_ptr()
    }

    /// Get the LID of the specified port.
    #[inline]
    pub fn lid(&self) -> u16 {
        self.inner.port_attr.lid
    }

    /// Get the port number passed by the user when opening this context.
    #[inline]
    pub fn port_num(&self) -> u8 {
        self.inner.port_num
    }

    /// Get the specified GID of the opened device.
    #[inline]
    pub fn gid(&self) -> Gid {
        self.inner.gid
    }

    /// Get the GID index passed by the user when opening this context.
    #[inline]
    pub fn gid_index(&self) -> u8 {
        self.inner.gid_index
    }

    /// Get the active path MTU of the specified port in bytes.
    #[inline]
    pub fn mtu(&self) -> usize {
        match self.inner.port_attr.active_mtu {
            ibv_mtu::IBV_MTU_256 => 256,
            ibv_mtu::IBV_MTU_512 => 512,
            ibv_mtu::IBV_MTU_1024 => 1024,
            ibv_mtu::IBV_MTU_2048 => 2048,
            ibv_mtu::IBV_MTU_4096 => 4096,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    /// Get the active path MTU of the specified port as an `ibv_mtu` value.
    #[inline]
    pub(crate) fn mtu_raw(&self) -> ibv_mtu::Type {
        self.inner.port_attr.active_mtu
    }

    /// Allocate a protection domain on this context.
    pub fn alloc_pd(&self) -> Result<Pd> {
        Pd::new(self.clone())
    }

    /// Create a completion queue on this context.
    pub fn create_cq(&self, capacity: u32) -> Result<Cq> {
        Cq::new(self.clone(), capacity)
    }
}
