pub mod ecs;

use std::{
    alloc,
    any::Any,
    io::Write,
    io::{self, Read},
    mem,
    ops::Deref,
    slice, todo,
};

use anyhow::{Result, anyhow};
use ecs::{Component, Fetch, WorldQuery};
use io::IoSlice;
use mem::ManuallyDrop;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub struct PluginBuilder {
    name: String,
    rpcs: Vec<Box<dyn Fn(&mut Vec<u8>)>>,
}

impl PluginBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            rpcs: Vec::new(),
        }
    }

    pub fn add_rpc<Args: DeserializeOwned, F: Fn(Args) + 'static>(mut self, rpc: F) -> Self {
        // TODO: write rpc layout to buffer
        self.rpcs.push(Box::new(move |buffer: &mut Vec<u8>| {
            // let args = bincode::deserialize_from(buffer.as_ref()).unwrap();
            // rpc(args);
        }));
        self
    }



    pub fn init(self) -> Result<Plugin> {
        let mut plugin = Plugin {
            buffer: Some(Box::new(Buffer::with_capacity(100_000))),
        };

        <(&u32, &mut u32) as Fetch>::access();

        let _: () = plugin.call_rpc("world_query", &<(&u32, &mut u64, &f32, &u32) as Fetch>::access())?;
        
        Ok(plugin)
    }
}

pub struct Plugin {
    buffer: Option<Box<Buffer>>,
}

impl Plugin {
    pub fn call_rpc<Args: Serialize + DeserializeOwned, R: Serialize + DeserializeOwned>(
        &mut self,
        name: &str,
        args: &Args,
    ) -> Result<R> {
        let mut buffer = self.buffer.take().ok_or(anyhow!("buffer not avialable."))?;
        buffer.clear();

        bincode::serialize_into(&mut buffer, name)?;
        bincode::serialize_into(&mut buffer, args)?;

        let buffer_ptr = Box::into_raw(buffer);
        unsafe { __quill_host_call(buffer_ptr) };
        let buffer = unsafe { Box::from_raw(buffer_ptr) };

        let result = bincode::deserialize_from(buffer.as_slice())?;

        self.buffer.replace(buffer);

        Ok(result)
    }
}

#[repr(C)]
#[derive(Debug)]
struct Buffer {
    ptr: *mut u8,
    cap: usize,
    len: usize,
}

impl Buffer {
    fn new() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            cap: 0,
            len: 0,
        }
    }

    fn with_capacity(capacity: usize) -> Self {
        Self::from(Vec::with_capacity(capacity))
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn reserve(&mut self, additional: usize) {
        let mut raw = unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) };
        raw.reserve(additional);
        self.update_from_vec(raw);
    }

    fn update_from_vec(&mut self, vec: Vec<u8>) {
        let mut me = ManuallyDrop::new(vec);
        self.ptr = me.as_mut_ptr();
        self.len = me.len();
        self.cap = me.capacity();
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        let mut raw = unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) };
        raw.extend_from_slice(other);
        self.update_from_vec(raw);
    }

    fn as_slice(&self) -> &[u8] {
        self
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        drop(unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) });
    }
}

impl From<Vec<u8>> for Buffer {
    fn from(raw: Vec<u8>) -> Self {
        let mut buffer = Self::new();
        buffer.update_from_vec(raw);
        buffer
    }
}

impl Write for Buffer {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.extend_from_slice(buf);
        Ok(buf.len())
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        let len = bufs.iter().map(|b| b.len()).sum();
        self.reserve(len);
        for buf in bufs {
            self.extend_from_slice(buf);
        }
        Ok(len)
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.extend_from_slice(buf);
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Deref for Buffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

extern "C" {
    fn __quill_host_call(buffer: *mut Buffer);
}

#[no_mangle]
extern "C" fn __quill_client_call() {}

#[no_mangle]
extern "C" fn __quill_buffer_reserve(buffer: *mut Buffer, additional: usize) {
    let mut buffer = unsafe { Box::from_raw(buffer) };
    buffer.reserve(additional);
    Box::leak(buffer);
}
