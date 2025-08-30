use log::info;
use redstone_computer_utilities::{ResponseOrErrorCode, Result, Script};

fn main() -> Result<()> {
    Script::new()
        .on_init(|_ctx| {
            info!("on_init is called!");
            ResponseOrErrorCode::Ok(serde_json::json!({}))
        })
        .on_execute(|_ctx| {
            info!("on_execute is called!");
            ResponseOrErrorCode::Ok(serde_json::json!({}))
        })
        .run()
}
