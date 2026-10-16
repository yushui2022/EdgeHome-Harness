fn main() -> anyhow::Result<()> {
    edgehome_kit::run_cli(edgehome_kit::starter::registry, edgehome_kit::starter::mock)
}
