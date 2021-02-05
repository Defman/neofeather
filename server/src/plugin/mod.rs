use std::{
    alloc::Layout,
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
    todo, u32, vec,
};

use anyhow::{anyhow, Result};
use bevy_ecs::{
    ComponentId, DynamicFetch, DynamicFetchResult, DynamicQuery, DynamicSystem, EntityBuilder,
    QueryAccess, StatefulQuery, TypeAccess, TypeInfo, World,
};
use bincode::DefaultOptions;
use fs::OpenOptions;
use io::IoSlice;
use mem::ManuallyDrop;
use quill::ecs::TypeLayout;
use wasmer::{
    import_namespace, imports, Array, FromToNativeWasmType, Function, HostEnvInitError, Instance,
    LazyInit, Memory, Module, NativeFunc, Store, Type, ValueType, WasmPtr, WasmTypeList, WasmerEnv,
    JIT, LLVM,
};
use wasmer_wasi::WasiState;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Default)]
struct PluginEnv<S> {
    memory: LazyInit<Memory>,
    buffer_reserve: LazyInit<NativeFunc<(WasmPtr<RawBuffer>, u32)>>,
    rpcs: Arc<Mutex<HashMap<String, Box<dyn Fn(&mut Buffer, &PluginEnv<S>) -> Result<()> + Send>>>>,
    state: Arc<Mutex<S>>,
    layouts: Arc<Mutex<Layouts>>,
}

impl<S: Send + Sync + 'static> Clone for PluginEnv<S> {
    fn clone(&self) -> Self {
        Self {
            memory: self.memory.clone(),
            buffer_reserve: self.buffer_reserve.clone(),
            rpcs: self.rpcs.clone(),
            state: self.state.clone(),
            layouts: Default::default(),
        }
    }
}

impl<S: Send + Sync + 'static> WasmerEnv for PluginEnv<S> {
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

impl<S: Send + Sync + 'static> PluginEnv<S> {
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

    fn add_rpc<
        'a,
        Args: Serialize + DeserializeOwned + 'static,
        R: Serialize + DeserializeOwned + 'static,
    >(
        &mut self,
        name: &str,
        callback: fn(&PluginEnv<S>, Args) -> R,
    ) -> Result<()> {
        self.rpcs
            .lock()
            .map_err(|_| anyhow!("could not lock rpcs"))?
            .insert(
                name.to_owned(),
                Box::new(move |mut buffer: &mut Buffer, env: &PluginEnv<S>| {
                    let (_, args): (String, Args) =
                        bincode::deserialize(buffer.as_slice()).unwrap();

                    let result = callback(env, args);
                    buffer.clear();
                    bincode::serialize_into(buffer, &result).unwrap();
                    Ok(())
                }),
            );
        Ok(())
    }

    fn call<Args: Serialize, R: DeserializeOwned>(&self, name: &str, args: Args) -> Result<R> {
        // TODO: requires access to buffer.
        todo!()
    }
}

pub struct Plugin {
    instance: Instance,
    env: PluginEnv<World>,
}

impl Plugin {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut env = PluginEnv::default();

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

        // env.add_rpc("players_push", |state, player: String| state.push(player))?;
        // // TODO: Return reference to state?
        // env.add_rpc("players", |state, ()| state.clone())?;

        env.add_rpc("world_spawn", |env, entity: quill::ecs::Entity| {
            let mut world = env.state.lock().unwrap();
            let mut layouts = env.layouts.lock().unwrap();

            let mut builder = EntityBuilder::new();
            for (layout, data) in entity.components {
                builder.add_dynamic(
                    TypeInfo::of_external(
                        layouts.external_id(&layout),
                        Layout::new::<Vec<u8>>(),
                        |_| (),
                    ),
                    data.as_slice(),
                );
            }
            world.spawn(builder.build());
        })?;

        env.add_rpc(
            "world_query",
            // TODO: world should not be the state but union(world, layouts)
            |env, access: quill::ecs::QueryAccess| {
                let world = env.state.lock().unwrap();
                let mut layouts = env.layouts.lock().unwrap();

                let query = access.query(&mut layouts).unwrap();
                let access = Default::default();
                let mut query: StatefulQuery<DynamicQuery, DynamicQuery> =
                    StatefulQuery::new(&world, &access, query);

                for entity in query.iter_mut() {
                    entity.immutable;
                    entity.mutable;
                }
            },
        )?;

        let instance = Instance::new(&module, &import_object)?;

        let start = instance.exports.get_function("_start")?;
        start.call(&[])?;

        Ok(Plugin { instance, env })
    }
}

#[derive(Default)]
pub struct Layouts {
    layouts: HashMap<quill::ecs::TypeLayout, u64>,
}

impl Layouts {
    pub fn component_id(&mut self, layout: &TypeLayout) -> ComponentId {
        ComponentId::ExternalId(self.external_id(layout))
    }

    pub fn external_id(&mut self, layout: &TypeLayout) -> u64 {
        if let Some(component_id) = self.layouts.get(&layout) {
            *component_id
        } else {
            let next = self.layouts.len() as u64;
            self.layouts.insert(layout.clone(), next);
            next
        }
    }
}

trait IntoBevyAccess {
    fn access(&self, layouts: &mut Layouts) -> Result<QueryAccess>;
    fn component_ids(&self) -> Result<Vec<ComponentId>>;

    fn query(&self, layouts: &mut Layouts) -> Result<DynamicQuery>;
}

impl IntoBevyAccess for quill::ecs::QueryAccess {
    fn access(&self, layouts: &mut Layouts) -> Result<QueryAccess> {
        use quill::ecs::QueryAccess::*;
        Ok(match self {
            None => QueryAccess::None,
            Read(layout) => QueryAccess::Read(layouts.component_id(layout), "??"),
            Write(layout) => QueryAccess::Write(layouts.component_id(layout), "??"),
            Optional(access) => {
                QueryAccess::optional(IntoBevyAccess::access(access.as_ref(), layouts)?)
            }
            With(layout, access) => QueryAccess::With(
                layouts.component_id(layout),
                Box::new(IntoBevyAccess::access(access.as_ref(), layouts)?),
            ),
            Without(layout, access) => QueryAccess::Without(
                layouts.component_id(layout),
                Box::new(IntoBevyAccess::access(access.as_ref(), layouts)?),
            ),
            Union(accesses) => QueryAccess::Union(
                accesses
                    .into_iter()
                    .map(|access| IntoBevyAccess::access(access, layouts))
                    .collect::<Result<Vec<QueryAccess>>>()?,
            ),
        })
    }

    fn component_ids(&self) -> Result<Vec<ComponentId>> {
        todo!()
    }

    fn query(&self, layouts: &mut Layouts) -> Result<DynamicQuery> {
        let mut query = DynamicQuery::default();
        query.access = self.access(layouts)?;

        // TODO: TypeInfo

        Ok(query)
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

fn __quill_host_call(env: &PluginEnv<World>, buffer_raw: WasmPtr<RawBuffer>) {
    let mut buffer = env.buffer(buffer_raw);

    let name: String = bincode::deserialize_from(buffer.as_slice()).unwrap();

    let rpcs = env.rpcs.lock().unwrap();
    let rpc = rpcs.get(&name).unwrap();

    rpc(&mut buffer, env).unwrap();
}
