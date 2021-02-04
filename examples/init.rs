use std::vec;

use quill::{
    PluginBuilder,
};

fn main() {
    PluginBuilder::new("hello world")
        .add_rpc(|name: String| println!("hello {}!", name))
        // .add_system(foo_system)
        .init()
        .expect("could not initlize plugin");
}

// fn foo_system(query: Query<(&Player, &Health)>) {
    
// }