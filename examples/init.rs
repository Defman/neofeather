use std::vec;

use quill::{PluginBuilder, ecs::Query};

fn main() {
    PluginBuilder::new("hello world")
        .add_rpc(|name: String| println!("hello {}!", name))
        // .add_system(foo_system)
        .init()
        .expect("could not initlize plugin");
}

fn foo_system(mut query: Query<(&(), &mut u32)>) {
    for (_, health) in query.iter_mut() {
        *health += 100;
    }
}