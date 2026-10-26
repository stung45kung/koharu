//! LLM-driven translation. Collects `text` from every text node on the page,
//! sends them through the loaded LLM as tagged blocks, writes the parsed
//! translations back via `UpdateNode { TextDataPatch { translation } }`.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{
    CharacterBook, NodeDataPatch, NodeId, NodePatch, Op, PageId, Scene, TextData, TextDataPatch,
};
use koharu_llm::Language;
use koharu_llm::prompt::system_prompt_intro;

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::text_nodes;

pub struct Model;

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let targets = collect_translation_targets(&ctx);
        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let sources: Vec<String> = targets.iter().map(|(_, s)| s.clone()).collect();
        // Tag `[i + 1]` is `targets[i]`, so the speaker map is built from the
        // exact nodes being sent (works for single-block and reordered runs).
        let node_ids: Vec<NodeId> = targets.iter().map(|(id, _)| *id).collect();
        let sections: Vec<String> = [
            ctx.characters.speaker_prompt_section(&node_ids),
            previous_page_context(ctx.scene, ctx.page, ctx.characters),
        ]
        .into_iter()
        .flatten()
        .collect();
        let system_prompt = effective_system_prompt(
            ctx.options.system_prompt.as_deref(),
            ctx.options.target_language.as_deref(),
            &sections,
        );
        let translations = ctx
            .llm
            .translate_texts(
                &sources,
                ctx.options.target_language.as_deref(),
                system_prompt.as_deref(),
            )
            .await?;

        let mut ops = Vec::with_capacity(targets.len());
        for ((node_id, _), translation) in targets.into_iter().zip(translations) {
            // A block the model skipped comes back empty; keep whatever
            // translation the block already had instead of wiping it.
            if translation.trim().is_empty() {
                tracing::warn!(node = %node_id, "LLM returned no translation for block; keeping existing text");
                continue;
            }
            ops.push(Op::UpdateNode {
                page: ctx.page,
                id: node_id,
                patch: NodePatch {
                    data: Some(NodeDataPatch::Text(TextDataPatch {
                        translation: Some(Some(translation)),
                        ..Default::default()
                    })),
                    transform: None,
                    visible: None,
                },
                prev: NodePatch::default(),
            });
        }
        Ok(ops)
    }
}

/// System prompt to send. Without extra sections this is exactly the user's
/// custom prompt (or `None` → provider default), i.e. unchanged behaviour.
/// Extra sections (speakers, previous-page context, ...) are appended to the
/// custom prompt, or to the default instructions when no custom prompt is
/// set. Block-tag rules are appended later by every LLM path, so they are not
/// repeated here.
fn effective_system_prompt(
    custom: Option<&str>,
    target_language: Option<&str>,
    sections: &[String],
) -> Option<String> {
    let custom = custom.filter(|p| !p.trim().is_empty());
    if sections.is_empty() {
        return custom.map(str::to_string);
    }
    let section = sections.join("\n\n");
    let base = match custom {
        Some(p) => p.trim_end().to_string(),
        None => {
            let lang = target_language
                .and_then(Language::parse)
                .unwrap_or(Language::English);
            system_prompt_intro(lang)
        }
    };
    Some(format!("{base}\n\n{section}"))
}

/// Lines of the previous page sent as context.
const CONTEXT_LINES: usize = 10;
/// Max characters of each source / translation line in the context.
const CONTEXT_LINE_CHARS: usize = 160;

/// The end of the previous page (speaker, source, translation) so the model
/// keeps names, terms, tone and running sentences consistent across pages.
/// Pipelines process pages in order, so in a multi-page run the previous
/// page is already translated when this page's turn comes.
fn previous_page_context(scene: &Scene, page: PageId, book: &CharacterBook) -> Option<String> {
    let index = scene.pages.get_index_of(&page)?;
    let (prev_id, _) = scene.pages.get_index(index.checked_sub(1)?)?;
    let lines: Vec<String> = text_nodes(scene, *prev_id)
        .into_iter()
        .filter_map(|(id, _, t)| {
            let source = t.text.as_deref()?.trim();
            if source.is_empty() {
                return None;
            }
            let speaker = book
                .speaker_of(id)
                .map(|c| one_line(&c.name))
                .unwrap_or_else(|| "?".to_string());
            let translation = t
                .translation
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| clip(s, CONTEXT_LINE_CHARS))
                .unwrap_or_else(|| "(not translated yet)".to_string());
            Some(format!(
                "- {speaker}: {} => {translation}",
                clip(source, CONTEXT_LINE_CHARS)
            ))
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    let skip = lines.len().saturating_sub(CONTEXT_LINES);
    Some(format!(
        "Context: the end of the previous page as `speaker: source => translation`. \
         It is for continuity only: do not translate it again and do not include it in \
         the output. Keep names, terms, each speaker's voice, and sentences that continue \
         from it consistent with these translations.\n{}",
        lines[skip..].join("\n")
    ))
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(s: &str, max: usize) -> String {
    let flat = one_line(s);
    match flat.char_indices().nth(max) {
        Some((idx, _)) => format!("{}…", &flat[..idx]),
        None => flat,
    }
}

fn collect_translation_targets(ctx: &EngineCtx<'_>) -> Vec<(NodeId, String)> {
    collect_translation_targets_from(ctx.scene, ctx.page, ctx.options.text_node_ids.as_deref())
}

fn collect_translation_targets_from(
    scene: &Scene,
    page: PageId,
    allowed_ids: Option<&[NodeId]>,
) -> Vec<(NodeId, String)> {
    text_nodes(scene, page)
        .into_iter()
        .filter(|(id, _, text_data)| should_translate(*id, text_data, allowed_ids))
        .filter_map(|(id, _, text_data)| text_data.text.as_ref().map(|source| (id, source.clone())))
        .collect()
}

fn should_translate(id: NodeId, text_data: &TextData, allowed_ids: Option<&[NodeId]>) -> bool {
    if let Some(ids) = allowed_ids
        && !ids.contains(&id)
    {
        return false;
    }
    text_data
        .text
        .as_ref()
        .is_some_and(|source| !source.trim().is_empty())
}

inventory::submit! {
    EngineInfo {
        id: "llm",
        name: "LLM",
        // `Speakers` only orders `speaker-guess` (when selected) before
        // translation so the guessed speakers reach the prompt.
        needs: &[Artifact::OcrText, Artifact::Speakers],
        produces: &[Artifact::Translations],
        load: |_runtime, _cpu| Box::pin(async move {
            Ok(Box::new(Model) as Box<dyn Engine>)
        }),
    }
}

#[cfg(test)]
mod tests {
    use koharu_core::{Node, NodeKind, Page, PageId, Scene, TextData, Transform};
    use uuid::Uuid;

    use super::*;

    fn node_id(value: u128) -> NodeId {
        NodeId(Uuid::from_u128(value))
    }

    fn page_id() -> PageId {
        PageId(Uuid::from_u128(1))
    }

    fn text_node(id: NodeId, text: Option<&str>) -> Node {
        Node {
            id,
            transform: Transform::default(),
            visible: true,
            kind: NodeKind::Text(TextData {
                text: text.map(str::to_string),
                ..Default::default()
            }),
        }
    }

    fn scene_with_texts(nodes: Vec<Node>) -> Scene {
        let page_id = page_id();
        let mut page = Page::new("page", 100, 100);
        page.id = page_id;
        page.nodes = nodes.into_iter().map(|node| (node.id, node)).collect();
        let mut scene = Scene::default();
        scene.pages.insert(page_id, page);
        scene
    }

    #[test]
    fn should_translate_only_requested_nodes() {
        let first = node_id(11);
        let second = node_id(22);
        let scene = scene_with_texts(vec![
            text_node(first, Some("first")),
            text_node(second, Some("second")),
        ]);
        let options = crate::PipelineRunOptions {
            text_node_ids: Some(vec![second]),
            ..Default::default()
        };

        let targets =
            collect_translation_targets_from(&scene, page_id(), options.text_node_ids.as_deref());

        assert_eq!(targets, vec![(second, "second".to_string())]);
    }

    #[test]
    fn prompt_is_unchanged_without_speakers() {
        assert_eq!(effective_system_prompt(None, Some("th-TH"), &[]), None);
        assert_eq!(
            effective_system_prompt(Some("แปลเป็นไทย"), None, &[]).as_deref(),
            Some("แปลเป็นไทย")
        );
        assert_eq!(effective_system_prompt(Some("   "), None, &[]), None);
    }

    #[test]
    fn speaker_section_is_appended_to_custom_prompt() {
        let out =
            effective_system_prompt(Some("แปลเป็นไทย  "), None, &["SPEAKERS".to_string()]).unwrap();
        assert_eq!(out, "แปลเป็นไทย\n\nSPEAKERS");
    }

    #[test]
    fn speaker_section_uses_default_intro_without_custom_prompt() {
        let out = effective_system_prompt(None, Some("en-US"), &["SPEAKERS".to_string()]).unwrap();
        assert!(out.starts_with("You are a professional manga translator."));
        assert!(out.ends_with("\n\nSPEAKERS"));
        // Tag rules are appended by the LLM layer; don't duplicate them.
        assert!(!out.contains("[1], [2]"));
    }

    #[test]
    fn previous_page_context_carries_source_translation_and_speaker() {
        use koharu_core::{Character, CharacterBook, CharacterId};

        let (a, b) = (node_id(1), node_id(2));
        let mut first = Page::new("p1", 100, 100);
        first.id = PageId(Uuid::from_u128(1));
        for (id, text, tl) in [(a, "海だー!", Some("ทะเลแล้ว!")), (b, "元気だね", None)]
        {
            first.nodes.insert(
                id,
                Node {
                    id,
                    transform: Transform::default(),
                    visible: true,
                    kind: NodeKind::Text(TextData {
                        text: Some(text.to_string()),
                        translation: tl.map(str::to_string),
                        ..Default::default()
                    }),
                },
            );
        }
        let mut second = Page::new("p2", 100, 100);
        second.id = PageId(Uuid::from_u128(2));
        let mut scene = Scene::default();
        scene.pages.insert(first.id, first);
        scene.pages.insert(second.id, second);

        let mut book = CharacterBook::default();
        let sui = CharacterId(Uuid::from_u128(7));
        book.set_characters(vec![Character {
            id: sui,
            name: "Sui".into(),
            description: String::new(),
        }]);
        book.assign(&[a], Some(sui)).unwrap();

        let ctx = previous_page_context(&scene, PageId(Uuid::from_u128(2)), &book).unwrap();
        assert!(ctx.contains("- Sui: 海だー! => ทะเลแล้ว!"));
        assert!(ctx.contains("- ?: 元気だね => (not translated yet)"));
        // First page has no previous page.
        assert!(previous_page_context(&scene, PageId(Uuid::from_u128(1)), &book).is_none());
    }

    #[test]
    fn several_sections_are_appended_in_order() {
        let out = effective_system_prompt(
            Some("แปลเป็นไทย"),
            None,
            &["SPEAKERS".to_string(), "CONTEXT".to_string()],
        )
        .unwrap();
        assert_eq!(out, "แปลเป็นไทย\n\nSPEAKERS\n\nCONTEXT");
    }

    #[test]
    fn speaker_tags_follow_translation_targets() {
        use koharu_core::{Character, CharacterBook, CharacterId};

        let (a, b, c) = (node_id(1), node_id(2), node_id(3));
        let scene = scene_with_texts(vec![
            text_node(a, Some("我が家の近くで")),
            text_node(b, Some("")),
            text_node(c, Some("こんな僻地で")),
        ]);
        let mut book = CharacterBook::default();
        let hood = CharacterId(Uuid::from_u128(500));
        book.set_characters(vec![Character {
            id: hood,
            name: "Hood".into(),
            description: "archaic".into(),
        }]);
        book.assign(&[c], Some(hood)).unwrap();

        // Block b has no OCR text, so c is sent as tag [2].
        let targets = collect_translation_targets_from(&scene, page_id(), None);
        let ids: Vec<NodeId> = targets.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![a, c]);
        let section = book.speaker_prompt_section(&ids).unwrap();
        assert!(section.contains("[1] unknown speaker"));
        assert!(section.contains("[2] S1"));
    }

    #[test]
    fn should_ignore_requested_nodes_without_ocr_text() {
        let blank = node_id(33);
        let scene = scene_with_texts(vec![
            text_node(blank, Some("   ")),
            text_node(node_id(44), Some("translated")),
        ]);
        let options = crate::PipelineRunOptions {
            text_node_ids: Some(vec![blank]),
            ..Default::default()
        };

        let targets =
            collect_translation_targets_from(&scene, page_id(), options.text_node_ids.as_deref());

        assert!(targets.is_empty());
    }
}
