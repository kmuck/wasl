//! Component assembly: core module + WIT (§ 13.5).

use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};

use crate::diag::Result;
use crate::wit::{Wit, internal};

/// Attaches the WIT of world `world` to the core module, then wraps it into a validated component.
pub fn build(mut module: Vec<u8>, wit: &Wit, world: &str) -> Result<Vec<u8>> {
    let world = wit.world(world)?;
    embed_component_metadata(
        &mut module,
        &wit.resolve,
        world,
        StringEncoding::UTF8,
        false,
    )
    .map_err(internal)?;
    let mut encoder = ComponentEncoder::default();
    encoder.module(&module).map_err(internal)?;
    encoder.validate(true);
    encoder.encode().map_err(internal)
}
