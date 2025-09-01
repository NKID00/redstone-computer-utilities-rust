use redstone_computer_utilities::{Result, Script};
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(LevelFilter::INFO.into()))
        .init();
    Script::new("hello")
        .on_init(async |mut ctx| {
            info!("on_init is called!");
            ctx.info("on_init is called!").await?;
            Ok(())
        })
        .on_execute(async |mut ctx, _args| {
            info!("on_execute is called!");
            ctx.info("on_execute is called!").await?;
            let gametime = ctx.query_gametime().await?;
            info!("gametime = {gametime}");
            ctx.info(format!("gametime = {gametime}")).await?;
            Ok(1)
        })
        .run()
        .await
}
