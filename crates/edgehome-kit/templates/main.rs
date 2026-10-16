mod tools;

fn main() -> anyhow::Result<()> {
    edgehome_kit::run_cli(tools::registry, tools::mock)
}
