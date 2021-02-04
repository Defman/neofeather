fn register_system(env: &PluginEnv) {
    let register_systems: () = env.read_buffer().unwrap();
    env.state.systems = register_systems.systems;
}

fn update_buffer(env: &PluginEnv, ptr: WasmPtr<u8, Array>, length: u32) {
    *env.buffer() = Buffer { ptr, length }
}
