mod foo;

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

        let version: String = plugin.call_rpc("version", &())?;
        println!("version: {}", version);

        plugin.call_rpc("hello", &"Defman".to_owned())?;

        plugin.call_rpc("players_push", &"Defman".to_owned())?;
        plugin.call_rpc("players_push", &"Bunny".to_owned())?;

        let players: Vec<String> = plugin.call_rpc("players", &())?;

        println!("players: {:?}", players);

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

        println!("??");

        bincode::serialize_into(&mut buffer, name)?;
        println!("????");
        bincode::serialize_into(&mut buffer, args)?;

        let mut other = Vec::new();
        bincode::serialize_into(&mut other, name)?;
        bincode::serialize_into(&mut other, name)?;

        println!("other buffer: {:?}", other.as_slice());

        println!("client buffer: {:?}", buffer.as_slice());

        let buffer_ptr = Box::into_raw(buffer);
        println!("buffer_ptr: {:?}", buffer_ptr);
        unsafe { __quill_host_call(buffer_ptr) };
        // HERE some where
        let buffer = unsafe { Box::from_raw(buffer_ptr) };

        println!("client buffer: {:?}", buffer.as_slice());

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
        Self::from(Vec::new())
    }

    fn with_capacity(capacity: usize) -> Self {
        Self::from(Vec::with_capacity(capacity))
    }

    fn reserve(&mut self, additional: usize) {
        let mut raw = unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) };
        raw.reserve(additional);
        *self = Self::from(raw);
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        let mut raw = unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) };
        // 1. allocate new array
        // 2. old[..len] copy to new[..len]
        // 3. other[..] copy to new[len..]
        println!("self {:?}", self);
        println!("raw before {:?}", raw.as_slice());
        raw.extend_from_slice(other);

        let raw_ptr = raw.as_mut_ptr();
        let raw_len = raw.len();
        let raw_cap = raw.capacity();

        println!("raw after {:?}", raw.as_slice());
        *self = Self::from(raw);
        println!("self {:?}", self);
        let raw = unsafe { Vec::from_raw_parts(raw_ptr, raw_len, raw_cap) };
        
        println!("raw after after: {:?}", raw.as_slice());
        Box::leak(raw.into_boxed_slice());
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
        // TODO: into raw parts
        let boxed: &'static mut [u8] = Box::leak(raw.into_boxed_slice());
        Self {
            ptr: boxed.as_mut_ptr(),
            cap: boxed.len(),
            len: boxed.len(),
        }
    }
}

impl Write for Buffer {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        println!("write {:?}", buf);
        self.extend_from_slice(buf);
        Ok(buf.len())
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        println!("write vectored {:?}", bufs);
        let len = bufs.iter().map(|b| b.len()).sum();
        self.reserve(len);
        for buf in bufs {
            self.extend_from_slice(buf);
        }
        Ok(len)
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        println!("write all {:?}", buf);
        self.extend_from_slice(buf);
        println!("finished write all");
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
