mod plugin;

fn main() {
    let result = plugin::Plugin::load("./target/wasm32-wasi/debug/examples/init.wasm");
    match result {
        Err(err) => println!("{:?}", err),
        Ok(_) => println!("all good?"),
    }
}

struct Server { 
    plugins: Vec<()>,
}