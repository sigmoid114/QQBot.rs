use kovi::tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let driver_config = kovi_milky::load_local_conf()?;
    let driver = kovi_milky::MilkyDriver::new(driver_config);

    let bot = kovi::build_bot!(driver; kovi_plugin_cmd);

    bot.run().await;
    Ok(())
}
