// `#[async_trait]` expands to `#[must_use]` fns returning a boxed future,
// which clippy >= 1.99 flags as `double_must_use`.
#![allow(clippy::double_must_use)]

pub mod codex;
pub mod provider;

pub use provider::{AiImageProvider, AiImageRequest, AiImageResult, AiInputImage};
