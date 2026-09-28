use std::{env, path::PathBuf};

use flick_ha::mock::{MockHa, MockScenario};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut scenario_path = None;
    let mut port = 0_u16;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => scenario_path = args.next().map(PathBuf::from),
            "--port" => {
                if let Some(value) = args.next() {
                    port = value.parse()?;
                }
            }
            _ => {}
        }
    }
    let scenario = if let Some(path) = scenario_path {
        MockScenario::from_path(path)?
    } else {
        MockScenario::default()
    };
    let (url, token, _handle) = MockHa::start_on_port(scenario, port).await?;
    println!("mock HA listening at {url} token={token}");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
