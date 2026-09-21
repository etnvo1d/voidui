// Derived from Zed/GPUI, revision e2534d2357a80795d2c372d31268748e7ee992e5.
// Licensed under Apache-2.0; see LICENSE-APACHE and UPSTREAM.md.
// Modified for the standalone voidui rendering package.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The fallback fonts that can be configured for a given font.
/// Fallback fonts family names are stored here.
#[derive(Default, Clone, Eq, PartialEq, Hash, Debug, Deserialize, Serialize, JsonSchema)]
pub struct FontFallbacks(pub Arc<Vec<String>>);

impl FontFallbacks {
    /// Get the fallback fonts family names
    pub fn fallback_list(&self) -> &[String] {
        self.0.as_slice()
    }

    /// Create a font fallback from a list of strings
    pub fn from_fonts(fonts: Vec<String>) -> Self {
        FontFallbacks(Arc::new(fonts))
    }
}
