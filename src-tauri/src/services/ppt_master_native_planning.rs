use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use super::native_design_system::NativeDesignSystemSpec;
use super::native_theme::{validate_visible_text_fragment, NativeThemeSpec};
use super::{default_theme, ContentBlock, Slide, SlidePlan, ThemeAllocation};

pub(super) const NATIVE_PLANNING_CHECKPOINT_FILE: &str = "native_planning_checkpoint.json";
pub(super) const NATIVE_PLANNING_CONTRACT_VERSION: &str =
    "pomegranate-native-planning-v1-outline-slide-spec";
pub(super) const NATIVE_PLANNING_MAX_ATTEMPTS: usize = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NativeSlideType {
    Cover,
    Section,
    Overview,
    Timeline,
    Process,
    Comparison,
    Data,
    Quote,
    Image,
    Profile,
    Content,
    Summary,
}

impl NativeSlideType {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Section => "section",
            Self::Overview => "overview",
            Self::Timeline => "timeline",
            Self::Process => "process",
            Self::Comparison => "comparison",
            Self::Data => "data",
            Self::Quote => "quote",
            Self::Image => "image",
            Self::Profile => "profile",
            Self::Content => "content",
            Self::Summary => "summary",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NativeLayoutIntent {
    Hero,
    Section,
    EditorialSplit,
    Timeline,
    Process,
    Comparison,
    DataFocus,
    QuoteFocus,
    ImageFocus,
    Profile,
    CardGrid,
    Summary,
}

impl NativeLayoutIntent {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Hero => "hero",
            Self::Section => "section",
            Self::EditorialSplit => "editorial_split",
            Self::Timeline => "timeline",
            Self::Process => "process",
            Self::Comparison => "comparison",
            Self::DataFocus => "data_focus",
            Self::QuoteFocus => "quote_focus",
            Self::ImageFocus => "image_focus",
            Self::Profile => "profile",
            Self::CardGrid => "card_grid",
            Self::Summary => "summary",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct DeckOutlineSlide {
    pub index: usize,
    pub narrative_role: String,
    pub title: String,
    pub core_message: String,
    pub slide_type: NativeSlideType,
    pub evidence_query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct DeckOutline {
    pub deck_title: String,
    pub objective: String,
    pub narrative: String,
    pub page_count: usize,
    pub slides: Vec<DeckOutlineSlide>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct SlideSpec {
    pub index: usize,
    pub title: String,
    pub subtitle: String,
    pub visible_content: Vec<String>,
    pub layout_intent: NativeLayoutIntent,
    pub visual_elements: Vec<String>,
    pub evidence: Vec<String>,
    pub speaker_notes: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NativePlanningErrorKind {
    JsonSyntax,
    SchemaValidation,
    FinishReason,
    Network,
}

impl NativePlanningErrorKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::JsonSyntax => "json_syntax",
            Self::SchemaValidation => "schema_validation",
            Self::FinishReason => "finish_reason",
            Self::Network => "network",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct NativePlanningContractError {
    pub kind: NativePlanningErrorKind,
    pub summary: String,
}

impl NativePlanningContractError {
    fn json(summary: impl Into<String>) -> Self {
        Self {
            kind: NativePlanningErrorKind::JsonSyntax,
            summary: summary.into(),
        }
    }

    fn schema(summary: impl Into<String>) -> Self {
        Self {
            kind: NativePlanningErrorKind::SchemaValidation,
            summary: summary.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativePlanningRequestMetric {
    pub request_id: String,
    pub phase: String,
    pub page_index: Option<usize>,
    pub attempt: usize,
    pub input_characters: usize,
    pub estimated_input_tokens: usize,
    pub output_characters: usize,
    pub elapsed_ms: u128,
    pub finish_reason: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub json_parse_result: String,
    pub schema_validation_result: String,
    pub error_kind: Option<String>,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativePlanningArtifactState {
    pub status: String,
    pub attempts: usize,
    pub path: String,
    pub last_error_kind: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: String,
}

impl NativePlanningArtifactState {
    fn pending(path: PathBuf) -> Self {
        Self {
            status: "pending".to_string(),
            attempts: 0,
            path: path.to_string_lossy().to_string(),
            last_error_kind: None,
            last_error: None,
            updated_at: now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativePlanningCheckpoint {
    pub schema_version: u32,
    pub contract_version: String,
    pub input_fingerprint: String,
    pub page_count: usize,
    pub status: String,
    pub outline: NativePlanningArtifactState,
    pub slide_specs: BTreeMap<String, NativePlanningArtifactState>,
    #[serde(default)]
    pub theme_spec: Option<NativeThemeSpec>,
    #[serde(default)]
    pub design_system_spec: Option<NativeDesignSystemSpec>,
    pub metrics: Vec<NativePlanningRequestMetric>,
    pub started_at: String,
    pub updated_at: String,
}

impl NativePlanningCheckpoint {
    pub(super) fn new(project: &Path, input_fingerprint: &str, page_count: usize) -> Self {
        let started_at = now();
        let slide_specs = (1..=page_count)
            .map(|index| {
                (
                    index.to_string(),
                    NativePlanningArtifactState::pending(slide_spec_path(project, index)),
                )
            })
            .collect();
        Self {
            schema_version: 1,
            contract_version: NATIVE_PLANNING_CONTRACT_VERSION.to_string(),
            input_fingerprint: input_fingerprint.to_string(),
            page_count,
            status: "running".to_string(),
            outline: NativePlanningArtifactState::pending(deck_outline_path(project)),
            slide_specs,
            theme_spec: None,
            design_system_spec: None,
            metrics: Vec::new(),
            started_at: started_at.clone(),
            updated_at: started_at,
        }
    }

    pub(super) fn slide_mut(
        &mut self,
        index: usize,
        project: &Path,
    ) -> &mut NativePlanningArtifactState {
        self.slide_specs
            .entry(index.to_string())
            .or_insert_with(|| {
                NativePlanningArtifactState::pending(slide_spec_path(project, index))
            })
    }

    pub(super) fn record_metric(&mut self, metric: NativePlanningRequestMetric) {
        self.metrics.push(metric);
        self.updated_at = now();
    }
}

pub(super) fn deck_outline_schema() -> Value {
    let slide_types = [
        "cover",
        "section",
        "overview",
        "timeline",
        "process",
        "comparison",
        "data",
        "quote",
        "image",
        "profile",
        "content",
        "summary",
    ];
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["deck_title", "objective", "narrative", "page_count", "slides"],
        "properties": {
            "deck_title": {"type": "string", "minLength": 1, "maxLength": 120},
            "objective": {"type": "string", "minLength": 1, "maxLength": 400},
            "narrative": {"type": "string", "minLength": 1, "maxLength": 800},
            "page_count": {"type": "integer", "minimum": 1, "maximum": 30},
            "slides": {
                "type": "array",
                "minItems": 1,
                "maxItems": 30,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["index", "narrative_role", "title", "core_message", "slide_type", "evidence_query"],
                    "properties": {
                        "index": {"type": "integer", "minimum": 1, "maximum": 30},
                        "narrative_role": {"type": "string", "minLength": 1, "maxLength": 80},
                        "title": {"type": "string", "minLength": 1, "maxLength": 100},
                        "core_message": {"type": "string", "minLength": 1, "maxLength": 300},
                        "slide_type": {"type": "string", "enum": slide_types},
                        "evidence_query": {"type": "string", "minLength": 1, "maxLength": 240}
                    }
                }
            }
        }
    })
}

pub(super) fn slide_spec_schema() -> Value {
    let layout_intents = [
        "hero",
        "section",
        "editorial_split",
        "timeline",
        "process",
        "comparison",
        "data_focus",
        "quote_focus",
        "image_focus",
        "profile",
        "card_grid",
        "summary",
    ];
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["index", "title", "subtitle", "visible_content", "layout_intent", "visual_elements", "evidence", "speaker_notes"],
        "properties": {
            "index": {"type": "integer", "minimum": 1, "maximum": 30},
            "title": {"type": "string", "minLength": 1, "maxLength": 100},
            "subtitle": {"type": "string", "maxLength": 160},
            "visible_content": {"type": "array", "minItems": 1, "maxItems": 6, "items": {"type": "string", "minLength": 1, "maxLength": 240}},
            "layout_intent": {"type": "string", "enum": layout_intents},
            "visual_elements": {"type": "array", "maxItems": 6, "items": {"type": "string", "minLength": 1, "maxLength": 160}},
            "evidence": {"type": "array", "minItems": 1, "maxItems": 8, "items": {"type": "string", "minLength": 1, "maxLength": 300}},
            "speaker_notes": {"type": "string", "maxLength": 1200}
        }
    })
}

pub(super) fn parse_deck_outline(
    raw: &str,
    expected_page_count: usize,
) -> Result<DeckOutline, NativePlanningContractError> {
    let outline: DeckOutline = parse_contract(raw, "DeckOutline")?;
    validate_deck_outline(&outline, expected_page_count)?;
    Ok(outline)
}

pub(super) fn parse_slide_spec(
    raw: &str,
    expected_index: usize,
) -> Result<SlideSpec, NativePlanningContractError> {
    let payload = deterministic_json_payload(raw)?;
    let mut value: Value = serde_json::from_str(payload)
        .map_err(|error| NativePlanningContractError::json(format!("SlideSpec: {error}")))?;
    normalize_layout_intent_alias(&mut value);
    let spec: SlideSpec = serde_json::from_value(value)
        .map_err(|error| NativePlanningContractError::schema(format!("SlideSpec: {error}")))?;
    validate_slide_spec(&spec, expected_index)?;
    Ok(spec)
}

fn normalize_layout_intent_alias(value: &mut Value) {
    if value.get("layout_intent").and_then(Value::as_str) == Some("overview") {
        value["layout_intent"] = Value::String("editorial_split".to_string());
    }
}

fn effective_layout_intent(
    slide_type: NativeSlideType,
    proposed: NativeLayoutIntent,
) -> NativeLayoutIntent {
    match slide_type {
        NativeSlideType::Overview => NativeLayoutIntent::EditorialSplit,
        _ => proposed,
    }
}

fn parse_contract<T: DeserializeOwned>(
    raw: &str,
    label: &str,
) -> Result<T, NativePlanningContractError> {
    let payload = deterministic_json_payload(raw)?;
    let value: Value = serde_json::from_str(payload)
        .map_err(|error| NativePlanningContractError::json(format!("{label}: {error}")))?;
    serde_json::from_value(value)
        .map_err(|error| NativePlanningContractError::schema(format!("{label}: {error}")))
}

fn deterministic_json_payload(raw: &str) -> Result<&str, NativePlanningContractError> {
    let trimmed = raw.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Err(NativePlanningContractError::json("empty response"));
    }
    let without_fence = if trimmed.starts_with("```") {
        let first_newline = trimmed
            .find('\n')
            .ok_or_else(|| NativePlanningContractError::json("unterminated markdown fence"))?;
        let body = &trimmed[first_newline + 1..];
        body.strip_suffix("```")
            .map(str::trim)
            .ok_or_else(|| NativePlanningContractError::json("unterminated markdown fence"))?
    } else {
        trimmed
    };
    if without_fence.starts_with('{') && without_fence.ends_with('}') {
        return Ok(without_fence);
    }
    let start = without_fence
        .find('{')
        .ok_or_else(|| NativePlanningContractError::json("response has no JSON object start"))?;
    let end = without_fence
        .rfind('}')
        .ok_or_else(|| NativePlanningContractError::json("response has no JSON object end"))?;
    if end <= start {
        return Err(NativePlanningContractError::json(
            "response JSON object range is invalid",
        ));
    }
    Ok(without_fence[start..=end].trim())
}

pub(super) fn validate_deck_outline(
    outline: &DeckOutline,
    expected_page_count: usize,
) -> Result<(), NativePlanningContractError> {
    if !(1..=30).contains(&expected_page_count) {
        return Err(NativePlanningContractError::schema(format!(
            "requested page_count {expected_page_count} is outside 1..=30"
        )));
    }
    check_string("deck_title", &outline.deck_title, 1, 120)?;
    check_string("objective", &outline.objective, 1, 400)?;
    check_string("narrative", &outline.narrative, 1, 800)?;
    if outline.page_count != expected_page_count || outline.slides.len() != expected_page_count {
        return Err(NativePlanningContractError::schema(format!(
            "page_count mismatch: expected={expected_page_count}, declared={}, slides={}",
            outline.page_count,
            outline.slides.len()
        )));
    }
    let mut titles = HashSet::new();
    let mut claims = HashSet::new();
    for (position, slide) in outline.slides.iter().enumerate() {
        let expected_index = position + 1;
        if slide.index != expected_index {
            return Err(NativePlanningContractError::schema(format!(
                "slide index must be unique and continuous: expected={expected_index}, actual={}",
                slide.index
            )));
        }
        check_string("narrative_role", &slide.narrative_role, 1, 80)?;
        check_string("slide.title", &slide.title, 1, 100)?;
        check_string("core_message", &slide.core_message, 1, 300)?;
        check_string("evidence_query", &slide.evidence_query, 1, 240)?;
        if !titles.insert(normalized_key(&slide.title)) {
            return Err(NativePlanningContractError::schema(format!(
                "duplicate slide title at index {}",
                slide.index
            )));
        }
        if !claims.insert(normalized_key(&slide.core_message)) {
            return Err(NativePlanningContractError::schema(format!(
                "duplicate core_message at index {}",
                slide.index
            )));
        }
    }
    if outline.slides.first().map(|slide| slide.slide_type) != Some(NativeSlideType::Cover) {
        return Err(NativePlanningContractError::schema(
            "slide 1 must use slide_type=cover",
        ));
    }
    if outline
        .slides
        .iter()
        .skip(1)
        .any(|slide| slide.slide_type == NativeSlideType::Cover)
    {
        return Err(NativePlanningContractError::schema(
            "slide_type=cover is only allowed at index 1",
        ));
    }
    Ok(())
}

pub(super) fn validate_slide_spec(
    spec: &SlideSpec,
    expected_index: usize,
) -> Result<(), NativePlanningContractError> {
    if spec.index != expected_index {
        return Err(NativePlanningContractError::schema(format!(
            "SlideSpec index mismatch: expected={expected_index}, actual={}",
            spec.index
        )));
    }
    check_string("title", &spec.title, 1, 100)?;
    check_string("subtitle", &spec.subtitle, 0, 160)?;
    check_string_list("visible_content", &spec.visible_content, 1, 6, 240)?;
    check_string_list("visual_elements", &spec.visual_elements, 0, 6, 160)?;
    check_string_list("evidence", &spec.evidence, 1, 8, 300)?;
    check_string("speaker_notes", &spec.speaker_notes, 0, 1200)?;
    Ok(())
}

fn check_string(
    field: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), NativePlanningContractError> {
    let length = value.trim().chars().count();
    if length < minimum || length > maximum {
        return Err(NativePlanningContractError::schema(format!(
            "{field} length must be {minimum}..={maximum}, actual={length}"
        )));
    }
    validate_visible_text_fragment(value).map_err(|error| {
        NativePlanningContractError::schema(format!("{field} text integrity failed: {error}"))
    })?;
    Ok(())
}

fn check_string_list(
    field: &str,
    values: &[String],
    minimum_items: usize,
    maximum_items: usize,
    maximum_chars: usize,
) -> Result<(), NativePlanningContractError> {
    if values.len() < minimum_items || values.len() > maximum_items {
        return Err(NativePlanningContractError::schema(format!(
            "{field} item count must be {minimum_items}..={maximum_items}, actual={}",
            values.len()
        )));
    }
    for (index, value) in values.iter().enumerate() {
        check_string(&format!("{field}[{index}]"), value, 1, maximum_chars)?;
    }
    Ok(())
}

fn normalized_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && !character.is_ascii_punctuation())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) struct NativeMaterialIndex {
    units: Vec<String>,
    raw_characters: usize,
}

impl NativeMaterialIndex {
    pub(super) fn new(raw_material: &str) -> Self {
        let mut units = Vec::new();
        let mut seen = HashSet::new();
        let mut current = String::new();
        for character in raw_material.chars() {
            current.push(character);
            if matches!(
                character,
                '。' | '！' | '？' | '；' | '\n' | '\r' | '.' | '!' | '?' | ';'
            ) {
                push_index_unit(&mut units, &mut seen, &current);
                current.clear();
            }
        }
        push_index_unit(&mut units, &mut seen, &current);
        Self {
            units,
            raw_characters: raw_material.chars().count(),
        }
    }

    pub(super) fn raw_characters(&self) -> usize {
        self.raw_characters
    }

    pub(super) fn unit_count(&self) -> usize {
        self.units.len()
    }

    pub(super) fn retrieve(
        &self,
        query: &str,
        page_index: usize,
        page_count: usize,
    ) -> Vec<String> {
        if self.units.is_empty() {
            return Vec::new();
        }
        let terms = retrieval_terms(query);
        let mut scored = self
            .units
            .iter()
            .enumerate()
            .map(|(index, unit)| {
                let lower = unit.to_lowercase();
                let score = terms
                    .iter()
                    .map(|term| lower.matches(term).count() * term.chars().count().max(1))
                    .sum::<usize>();
                (score, index, unit)
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        let mut selected = scored
            .iter()
            .filter(|(score, _, _)| *score > 0)
            .take(12)
            .map(|(_, _, unit)| (*unit).clone())
            .collect::<Vec<_>>();
        if selected.len() < 4 {
            let chunk = (self.units.len() + page_count.saturating_sub(1)) / page_count.max(1);
            let start = page_index
                .saturating_sub(1)
                .saturating_mul(chunk)
                .min(self.units.len().saturating_sub(1));
            for unit in self.units.iter().skip(start).take(8) {
                if !selected.contains(unit) {
                    selected.push(unit.clone());
                }
            }
        }
        let mut characters = 0usize;
        selected
            .into_iter()
            .filter(|unit| {
                let next = characters + unit.chars().count();
                if next > 4_800 {
                    false
                } else {
                    characters = next;
                    true
                }
            })
            .collect()
    }
}

fn push_index_unit(units: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches(|character: char| {
        character.is_ascii_punctuation() || "，。；：、！？（）()[]【】".contains(character)
    });
    let length = cleaned.chars().count();
    if (8..=600).contains(&length) {
        let key = normalized_key(cleaned);
        if seen.insert(key) {
            units.push(cleaned.to_string());
        }
    }
}

fn retrieval_terms(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let mut terms = HashSet::new();
    for word in lower.split(|character: char| {
        character.is_whitespace()
            || character.is_ascii_punctuation()
            || "，。；：、！？（）".contains(character)
    }) {
        if word.chars().count() >= 2 {
            terms.insert(word.to_string());
        }
    }
    let cjk = lower
        .chars()
        .filter(|character| ('\u{3400}'..='\u{9fff}').contains(character))
        .collect::<Vec<_>>();
    for window in cjk.windows(2) {
        terms.insert(window.iter().collect());
    }
    terms.into_iter().collect()
}

pub(super) fn assemble_slide_plan(
    outline: &DeckOutline,
    specs: &[SlideSpec],
    audience: &str,
    style: &str,
) -> SlidePlan {
    let slides = outline
        .slides
        .iter()
        .zip(specs.iter())
        .map(|(outline_slide, spec)| {
            let layout_intent =
                effective_layout_intent(outline_slide.slide_type, spec.layout_intent);
            let layout = layout_intent.as_str().to_string();
            let (relation, chart_type, page_rhythm) = layout_contract(layout_intent);
            let must_avoid = outline
                .slides
                .iter()
                .filter(|candidate| candidate.index != outline_slide.index)
                .map(|candidate| candidate.title.clone())
                .take(4)
                .collect::<Vec<_>>();
            let content_blocks = spec
                .visible_content
                .iter()
                .enumerate()
                .map(|(index, text)| ContentBlock {
                    label: format!("要点 {}", index + 1),
                    text: text.clone(),
                    detail: String::new(),
                })
                .collect::<Vec<_>>();
            Slide {
                page: spec.index,
                page_index: spec.index,
                page_id: format!("P{:02}", spec.index),
                slide_type: outline_slide.slide_type.as_str().to_string(),
                layout,
                title: spec.title.clone(),
                subtitle: spec.subtitle.clone(),
                bullets: spec.visible_content.clone(),
                visual_hint: spec.visual_elements.join("；"),
                page_theme: outline_slide.narrative_role.clone(),
                main_claim: outline_slide.core_message.clone(),
                core_message: outline_slide.core_message.clone(),
                content_scope: outline_slide.evidence_query.clone(),
                content_blocks,
                evidence: spec.evidence.clone(),
                relation: relation.to_string(),
                density: page_rhythm.to_string(),
                visual_intent: spec.visual_elements.join("；"),
                must_include: spec.visible_content.clone(),
                must_avoid,
                page_rhythm: page_rhythm.to_string(),
                chart_ref: chart_type.to_string(),
                chart_type: chart_type.to_string(),
                file_stem: format!("slide_{:02}", spec.index),
                speaker_note: spec.speaker_notes.clone(),
            }
        })
        .collect::<Vec<_>>();
    let theme_allocation = outline
        .slides
        .iter()
        .map(|slide| ThemeAllocation {
            page_id: format!("P{:02}", slide.index),
            assigned_theme: slide.narrative_role.clone(),
            exclusive_scope: slide.evidence_query.clone(),
        })
        .collect();
    SlidePlan {
        title: outline.deck_title.clone(),
        subtitle: outline.objective.clone(),
        audience: audience.to_string(),
        style: style.to_string(),
        theme: default_theme(),
        theme_allocation,
        slides,
    }
}

fn layout_contract(intent: NativeLayoutIntent) -> (&'static str, &'static str, &'static str) {
    match intent {
        NativeLayoutIntent::Timeline => ("timeline", "timeline", "dense"),
        NativeLayoutIntent::Process => ("process", "process_flow", "dense"),
        NativeLayoutIntent::Comparison => ("compare", "compare", "dense"),
        NativeLayoutIntent::DataFocus => ("category", "kpi_cards", "balanced"),
        NativeLayoutIntent::Summary => ("summary", "summary", "balanced"),
        NativeLayoutIntent::Hero | NativeLayoutIntent::QuoteFocus => ("none", "none", "anchor"),
        NativeLayoutIntent::Section => ("none", "none", "breathing"),
        NativeLayoutIntent::Profile => ("category", "labeled_card", "balanced"),
        NativeLayoutIntent::ImageFocus => ("category", "labeled_card", "breathing"),
        NativeLayoutIntent::EditorialSplit | NativeLayoutIntent::CardGrid => {
            ("category", "cards", "balanced")
        }
    }
}

pub(super) fn checkpoint_path(project: &Path) -> PathBuf {
    project.join(NATIVE_PLANNING_CHECKPOINT_FILE)
}

pub(super) fn deck_outline_path(project: &Path) -> PathBuf {
    project.join("deck_outline.json")
}

pub(super) fn slide_spec_path(project: &Path, index: usize) -> PathBuf {
    project
        .join("slide_specs")
        .join(format!("slide-{index:02}.json"))
}

pub(super) fn load_or_create_checkpoint(
    project: &Path,
    input_fingerprint: &str,
    page_count: usize,
) -> Result<NativePlanningCheckpoint, String> {
    let path = checkpoint_path(project);
    if !path.is_file() {
        return Ok(NativePlanningCheckpoint::new(
            project,
            input_fingerprint,
            page_count,
        ));
    }
    let checkpoint: NativePlanningCheckpoint = read_json(&path)?;
    if checkpoint.schema_version != 1
        || checkpoint.contract_version != NATIVE_PLANNING_CONTRACT_VERSION
        || checkpoint.input_fingerprint != input_fingerprint
        || checkpoint.page_count != page_count
    {
        return Err(format!(
            "native planning checkpoint is incompatible with current input: {}",
            path.display()
        ));
    }
    Ok(checkpoint)
}

pub(super) fn persist_checkpoint(
    project: &Path,
    checkpoint: &NativePlanningCheckpoint,
) -> Result<(), String> {
    write_json_atomic(&checkpoint_path(project), checkpoint)
}

pub(super) fn read_outline(project: &Path, page_count: usize) -> Result<DeckOutline, String> {
    let raw = fs::read_to_string(deck_outline_path(project))
        .map_err(|error| format!("read deck outline failed: {error}"))?;
    parse_deck_outline(&raw, page_count)
        .map_err(|error| format!("{}: {}", error.kind.as_str(), error.summary))
}

pub(super) fn read_slide_spec(project: &Path, index: usize) -> Result<SlideSpec, String> {
    let raw = fs::read_to_string(slide_spec_path(project, index))
        .map_err(|error| format!("read SlideSpec P{index:02} failed: {error}"))?;
    parse_slide_spec(&raw, index)
        .map_err(|error| format!("{}: {}", error.kind.as_str(), error.summary))
}

pub(super) fn write_outline(project: &Path, outline: &DeckOutline) -> Result<(), String> {
    write_json_atomic(&deck_outline_path(project), outline)
}

pub(super) fn write_slide_spec(
    project: &Path,
    index: usize,
    spec: &SlideSpec,
) -> Result<(), String> {
    write_json_atomic(&slide_spec_path(project, index), spec)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("read JSON failed: {} ({error})", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("parse JSON failed: {} ({error})", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create directory failed: {} ({error})", parent.display()))?;
    }
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("serialize JSON failed: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)
        .map_err(|error| format!("create temporary JSON failed: {} ({error})", temp.display()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("write temporary JSON failed: {} ({error})", temp.display()))?;
    drop(file);
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            format!("remove previous JSON failed: {} ({error})", path.display())
        })?;
    }
    fs::rename(&temp, path)
        .map_err(|error| format!("commit JSON failed: {} ({error})", path.display()))
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_project(label: &str) -> PathBuf {
        let unique = format!(
            "pomegranate-native-planning-{label}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn outline() -> DeckOutline {
        DeckOutline {
            deck_title: "测试演示".to_string(),
            objective: "建立清晰叙事".to_string(),
            narrative: "从背景到结论".to_string(),
            page_count: 2,
            slides: vec![
                DeckOutlineSlide {
                    index: 1,
                    narrative_role: "开场".to_string(),
                    title: "封面".to_string(),
                    core_message: "建立主题".to_string(),
                    slide_type: NativeSlideType::Cover,
                    evidence_query: "主题人物".to_string(),
                },
                DeckOutlineSlide {
                    index: 2,
                    narrative_role: "总结".to_string(),
                    title: "影响".to_string(),
                    core_message: "总结长期影响".to_string(),
                    slide_type: NativeSlideType::Summary,
                    evidence_query: "影响 评价".to_string(),
                },
            ],
        }
    }

    fn spec(index: usize) -> SlideSpec {
        SlideSpec {
            index,
            title: format!("Page {index}"),
            subtitle: String::new(),
            visible_content: vec![format!("Evidence-backed point for page {index}")],
            layout_intent: NativeLayoutIntent::Summary,
            visual_elements: vec![],
            evidence: vec![format!("Local source fact for page {index}")],
            speaker_notes: String::new(),
        }
    }

    #[test]
    fn outline_rejects_unknown_fields_as_schema_error() {
        let mut value = serde_json::to_value(outline()).unwrap();
        value["unknown"] = json!(true);
        let error = parse_deck_outline(&value.to_string(), 2).unwrap_err();
        assert_eq!(error.kind, NativePlanningErrorKind::SchemaValidation);
    }

    #[test]
    fn malformed_json_is_not_repaired() {
        let error = parse_deck_outline(r#"{"deck_title":"x" "slides":[]}"#, 2).unwrap_err();
        assert_eq!(error.kind, NativePlanningErrorKind::JsonSyntax);
    }

    #[test]
    fn outline_requires_continuous_unique_indices() {
        let mut value = outline();
        value.slides[1].index = 3;
        let error = validate_deck_outline(&value, 2).unwrap_err();
        assert_eq!(error.kind, NativePlanningErrorKind::SchemaValidation);
        assert!(error.summary.contains("continuous"));
    }

    #[test]
    fn slide_spec_enforces_lengths_and_expected_index() {
        let spec = SlideSpec {
            index: 2,
            title: "影响".to_string(),
            subtitle: String::new(),
            visible_content: vec!["长期影响".to_string()],
            layout_intent: NativeLayoutIntent::Summary,
            visual_elements: vec![],
            evidence: vec!["来源片段".to_string()],
            speaker_notes: String::new(),
        };
        validate_slide_spec(&spec, 2).unwrap();
        assert!(validate_slide_spec(&spec, 1).is_err());
    }

    #[test]
    fn overview_layout_alias_maps_to_existing_editorial_split_intent() {
        let mut value = serde_json::to_value(spec(1)).unwrap();
        value["layout_intent"] = json!("overview");

        let parsed = parse_slide_spec(&value.to_string(), 1).unwrap();

        assert_eq!(parsed.layout_intent, NativeLayoutIntent::EditorialSplit);
    }

    #[test]
    fn overview_slide_type_deterministically_uses_editorial_split() {
        let mut outline = outline();
        outline.slides[0].slide_type = NativeSlideType::Overview;
        let specs = vec![spec(1), spec(2)];

        let plan = assemble_slide_plan(&outline, &specs, "general", "default");

        assert_eq!(plan.slides[0].layout, "editorial_split");
        assert_eq!(plan.slides[1].layout, "summary");
    }

    #[test]
    fn assembled_plan_keeps_every_visible_content_unit_for_executor() {
        let outline = outline();
        let mut second = spec(2);
        second.visible_content = (1..=6).map(|index| format!("事实 {index}")).collect();
        let specs = vec![spec(1), second];

        let plan = assemble_slide_plan(&outline, &specs, "general", "default");

        assert_eq!(plan.slides[1].must_include.len(), 6);
        assert_eq!(plan.slides[1].page_rhythm, "balanced");
        assert_eq!(plan.slides[1].density, "balanced");
    }

    #[test]
    fn deterministic_cleanup_only_strips_fence_bom_and_outer_text() {
        let raw = format!(
            "\u{feff}explanation\n```json\n{}\n```",
            serde_json::to_string(&outline()).unwrap()
        );
        let parsed = parse_deck_outline(&raw, 2).unwrap();
        assert_eq!(parsed, outline());
    }

    #[test]
    fn material_retrieval_returns_page_specific_subset() {
        let raw = "早年在湖南求学。革命时期开展武装斗争。新中国成立后推动国家建设。晚年经历政治运动。思想著作产生长期影响。";
        let index = NativeMaterialIndex::new(raw);
        let selected = index.retrieve("新中国 建设", 3, 5);
        assert!(selected.iter().any(|unit| unit.contains("国家建设")));
        assert!(
            selected
                .iter()
                .map(|unit| unit.chars().count())
                .sum::<usize>()
                < raw.chars().count()
        );
    }

    #[test]
    fn schemas_forbid_unknown_properties() {
        assert_eq!(deck_outline_schema()["additionalProperties"], json!(false));
        assert_eq!(slide_spec_schema()["additionalProperties"], json!(false));
        assert_eq!(
            deck_outline_schema()["properties"]["slides"]["items"]["additionalProperties"],
            json!(false)
        );
    }

    #[test]
    fn checkpoint_rejects_changed_input_fingerprint() {
        let project = test_project("fingerprint");
        let checkpoint = NativePlanningCheckpoint::new(&project, "fingerprint-a", 2);
        persist_checkpoint(&project, &checkpoint).unwrap();

        let error = load_or_create_checkpoint(&project, "fingerprint-b", 2).unwrap_err();
        assert!(error.contains("incompatible with current input"));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn validated_slide_specs_are_read_and_reused_independently() {
        let project = test_project("reuse");
        write_outline(&project, &outline()).unwrap();
        write_slide_spec(&project, 1, &spec(1)).unwrap();
        let mut checkpoint = NativePlanningCheckpoint::new(&project, "same-input", 2);
        checkpoint.outline.status = "validated".to_string();
        checkpoint.slide_mut(1, &project).status = "validated".to_string();
        checkpoint.slide_mut(2, &project).status = "failed".to_string();
        persist_checkpoint(&project, &checkpoint).unwrap();

        let loaded = load_or_create_checkpoint(&project, "same-input", 2).unwrap();
        assert_eq!(loaded.slide_specs["1"].status, "validated");
        assert_eq!(loaded.slide_specs["2"].status, "failed");
        assert_eq!(read_slide_spec(&project, 1).unwrap(), spec(1));
        assert!(read_slide_spec(&project, 2).is_err());
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn corrupt_checkpoint_is_reported_instead_of_silently_reused() {
        let project = test_project("corrupt");
        fs::write(checkpoint_path(&project), b"{not-json").unwrap();

        let error = load_or_create_checkpoint(&project, "same-input", 2).unwrap_err();
        assert!(error.contains("parse JSON failed"));
        fs::remove_dir_all(project).unwrap();
    }
}
