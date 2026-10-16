//! Three bounded, side-effect-free tools for real-model task evaluation.
//! The converter is a real calculation; the document/sensors are starter fixtures.
use edgehome_runtime::{ToolError, TypedTool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Unit {
    Mm,
    Cm,
    M,
}
impl Unit {
    fn scale(&self) -> f64 {
        match self {
            Self::Mm => 1.0,
            Self::Cm => 10.0,
            Self::M => 1000.0,
        }
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LengthArgs {
    value: f64,
    from: Unit,
    to: Unit,
}

fn main() -> anyhow::Result<()> {
    edgehome_kit::run_cli(
        || {
            let mut tools = edgehome_kit::starter::registry()?;
            tools.register(TypedTool::<LengthArgs, _, _>::new(
            "length.convert", "Convert a length between mm (millimeters), cm (centimeters), and m (meters). Requires the numeric value, source unit (from), and destination unit (to). No default values or units.",
            |a| if a.value.is_finite() && a.value.abs() <= 1_000_000.0 { Ok(()) } else { Err(ToolError::Denied) },
            |a, _| Ok(json!({"value":a.value*a.from.scale()/a.to.scale(),"unit":a.to})),
        ))?;
            Ok(tools)
        },
        edgehome_kit::starter::mock,
    )
}
