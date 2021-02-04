use std::{
    array::TryFromSliceError,
    borrow::BorrowMut,
    cell::{Cell, UnsafeCell},
    collections::HashMap,
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    mem,
    ops::{Deref, DerefMut},
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    todo, u32,
};

use anyhow::{anyhow, Result};
use fs::OpenOptions;
use io::IoSlice;
use mem::ManuallyDrop;
use wasmer::{
    import_namespace, imports, Array, FromToNativeWasmType, Function, HostEnvInitError, Instance,
    LazyInit, Memory, Module, NativeFunc, Store, Type, ValueType, WasmPtr, WasmTypeList, WasmerEnv,
    JIT, LLVM,
};
use wasmer_wasi::WasiState;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Clone, Default)]
struct PluginEnv<S> {
    memory: LazyInit<Memory>,
    buffer_reserve: LazyInit<NativeFunc<(WasmPtr<RawBuffer>, u32)>>,
    rpcs: Arc<Mutex<HashMap<String, Box<dyn Fn(&mut Buffer, &mut S) + Send>>>>,
    state: Arc<Mutex<S>>,
}

impl<S: Clone + Send + Sync> WasmerEnv for PluginEnv<S> {
    fn init_with_instance(&mut self, instance: &Instance) -> Result<(), HostEnvInitError> {
        let memory = instance.exports.get_memory("memory")?;
        self.memory.initialize(memory.clone());
        self.buffer_reserve.initialize(
            instance
                .exports
                .get_native_function("__quill_buffer_reserve")?,
        );
        Ok(())
    }
}

impl<S: Clone + Send> PluginEnv<S> {
    fn memory(&self) -> &Memory {
        // TODO: handle errors.
        self.memory.get_ref().unwrap()
    }

    fn buffer_reserve(&self) -> &NativeFunc<(WasmPtr<RawBuffer>, u32)> {
        self.buffer_reserve.get_ref().unwrap()
    }

    fn buffer(&self, raw: WasmPtr<RawBuffer>) -> Buffer {
        Buffer {
            memory: self.memory(),
            reserve: self.buffer_reserve(),
            raw,
        }
    }
}

pub struct Plugin {
    instance: Instance,
    env: PluginEnv<Vec<String>>,
}

impl Plugin {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let env = PluginEnv::default();

        let store = Store::new(&JIT::new(LLVM::default()).engine());

        let module = Module::from_file(&store, &path)?;

        let mut wasi_env = WasiState::new(
            path.as_ref()
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("unkown"),
        )
        .finalize()?;

        let mut import_object = wasi_env.import_object(&module)?;
        import_object.register(
            "env",
            import_namespace!({
                "__quill_host_call" => Function::new_native_with_env(&store, env.clone(), __quill_host_call),
            }),
        );

        let instance = Instance::new(&module, &import_object)?;

        {
            let mut rpcs = env.rpcs.lock().unwrap();

            rpcs.insert(
                "version".to_owned(),
                Box::new(|buffer: &mut Buffer, state| {
                    println!("....");
                    // Read all arguments of call
                    let mut reader = buffer.as_slice();
                    let _: String = bincode::deserialize_from(&mut reader).unwrap();
                    println!(".");
                    // here you could read all the args
                    // Clear the buffer to make place for the return value
                    println!("..");
                    buffer.clear();
                    println!("....");
                    bincode::serialize_into(buffer, "1.16").unwrap();
                    println!(".....");
                }),
            );

            rpcs.insert(
                "hello".to_owned(),
                Box::new(|buffer: &mut Buffer, state| {
                    let mut reader = buffer.as_slice();
                    let _: String = bincode::deserialize_from(&mut reader).unwrap();
                    let name: String = bincode::deserialize_from(&mut reader).unwrap();
                    println!("hello {}", name);
                    buffer.clear();
                }),
            );

            rpcs.insert(
                "players_push".to_owned(),
                Box::new(|buffer: &mut Buffer, state| {
                    let mut reader = buffer.as_slice();
                    let _: String = bincode::deserialize_from(&mut reader).unwrap();
                    let player_name: String = bincode::deserialize_from(&mut reader).unwrap();
                    state.push(player_name);
                    println!("state: {:?}", state);
                    buffer.clear();
                }),
            );

            rpcs.insert(
                "players".to_owned(),
                Box::new(|mut buffer: &mut Buffer, state: &mut Vec<String>| {
                    let mut reader = buffer.as_slice();
                    let _: String = bincode::deserialize_from(&mut reader).unwrap();
                    buffer.clear();
                    // println!("host buffer cleared: {:?}", buffer.as_slice());
                    bincode::serialize_into(&mut buffer, state).unwrap();
                    // println!("host buffer {:?}", buffer.as_slice());
                    // println!("expected buffer {:?}", bincode::serialize(state));
                }),
            );
        }

        let start = instance.exports.get_function("_start")?;
        start.call(&[])?;

        Ok(Plugin { instance, env })
    }

    fn call<Args: Serialize, R: DeserializeOwned>(&self, name: &str, args: Args) -> Result<R> {
        let client_rpc = self
            .env
            .rpcs
            .lock()
            .map_err(|_| anyhow!("could not get lock on client rpcs"))?
            .get(name)
            .ok_or(anyhow!("error"))?;
        // let buffer = self.buffer_mut()?;
        todo!()
    }
}

struct Buffer<'a> {
    memory: &'a Memory,
    // fn reserve(ptr: WasmPtr<u8, Array>, cap: u32, len: u32, additional: u32)
    reserve: &'a NativeFunc<(WasmPtr<RawBuffer>, u32)>,
    raw: WasmPtr<RawBuffer>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RawBuffer {
    ptr: WasmPtr<u8, Array>,
    cap: u32,
    len: u32,
}

unsafe impl ValueType for RawBuffer {}

impl<'a> Buffer<'a> {
    fn reserve(&mut self, additional: u32) {
        let raw = self.raw.deref(self.memory).unwrap().get();
        if raw.cap < raw.len + additional {
            self.reserve.call(self.raw, additional).unwrap();
        }
    }

    fn clear(&mut self) {
        let raw_cell = self.raw.deref(self.memory).unwrap();
        raw_cell.set(RawBuffer {
            len: 0,
            ..raw_cell.get()
        })
    }

    fn push(&mut self, byte: u8) {
        self.extend_from_slice(&[byte]);
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        self.reserve(other.len() as u32);
        let raw_cell = self.raw.deref(self.memory).unwrap();
        let raw = raw_cell.get();
        raw.ptr
            .deref(self.memory, raw.len, raw.cap)
            .unwrap()
            .into_iter()
            .zip(other.iter())
            .for_each(|(cell, value)| cell.set(*value));
        raw_cell.set(RawBuffer {
            len: raw.len + other.len() as u32,
            ..raw
        });
    }

    fn as_slice(&self) -> &[u8] {
        self
    }
}

impl<'a> Write for Buffer<'a> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.extend_from_slice(buf);
        Ok(buf.len())
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        let len = bufs.iter().map(|b| b.len() as u32).sum();
        self.reserve(len);
        for buf in bufs {
            self.extend_from_slice(buf);
        }
        Ok(len as usize)
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

impl<'a> Deref for Buffer<'a> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        let raw = self.raw.deref(self.memory).unwrap().get();
        unsafe { mem::transmute(raw.ptr.deref(self.memory, 0, raw.len).unwrap()) }
    }
}

impl<'a> AsRef<[u8]> for Buffer<'a> {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

fn __quill_host_call(env: &PluginEnv<Vec<String>>, buffer_raw: WasmPtr<RawBuffer>) {
    println!("buffer: {:?}", buffer_raw);
    let mut buffer = env.buffer(buffer_raw);

    println!("host buffer: {:?}", buffer.as_slice());
    // HERE

    let name: String = bincode::deserialize_from(buffer.as_slice()).unwrap();

    println!("1");

    let rpcs = env.rpcs.lock().unwrap();
    println!("2");
    let rpc = rpcs.get(&name).unwrap();
    println!("3");

    let mut state = env.state.lock().unwrap();
    
    println!("4");

    rpc(&mut buffer, &mut state);
    println!("5");
    // Drop buffer here, bad... manual drop????
}
