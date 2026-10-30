//! OCR text that an LLM OCR check replaced, per text block.
//!
//! Stored beside the scene in `.khrproj/ocr_fixes.toml` (not inside
//! `scene.bin`, whose positional binary format cannot take a new field
//! without breaking existing projects). The UI shows a "fixed" badge with
//! the original text and can restore it. Entries whose node no longer
//! exists are simply ignored.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::scene::NodeId;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OcrFixes {
    /// Text node → the OCR text before the LLM check changed it.
    #[serde(default)]
    pub originals: IndexMap<NodeId, String>,
}

impl OcrFixes {
    pub fn original(&self, node: NodeId) -> Option<&str> {
        self.originals.get(&node).map(String::as_str)
    }

    /// Remember `original` for `node`, unless the node already has one: a
    /// block checked again keeps the first OCR reading.
    pub fn record(&mut self, node: NodeId, original: String) {
        self.originals.entry(node).or_insert(original);
    }

    /// Drop the entries of `nodes` (OCR ran again, or the user restored).
    pub fn forget(&mut self, nodes: &[NodeId]) {
        self.originals.retain(|id, _| !nodes.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keeps_the_first_original() {
        let node = NodeId::new();
        let mut fixes = OcrFixes::default();
        fixes.record(node, "first".into());
        fixes.record(node, "second".into());
        assert_eq!(fixes.original(node), Some("first"));
    }

    #[test]
    fn forget_drops_only_the_given_nodes() {
        let (a, b) = (NodeId::new(), NodeId::new());
        let mut fixes = OcrFixes::default();
        fixes.record(a, "a".into());
        fixes.record(b, "b".into());
        fixes.forget(&[a]);
        assert_eq!(fixes.original(a), None);
        assert_eq!(fixes.original(b), Some("b"));
    }
}
