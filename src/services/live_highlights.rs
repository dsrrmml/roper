use std::collections::{HashMap, HashSet};

pub const REPEAT_BUCKETS: usize = 6;
pub const STRUCTURE_BUCKETS: usize = 6;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HighlightRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepeatHighlight {
    pub range: HighlightRange,
    pub bucket: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepeatWarningKind {
    ScatteredWeakWord,
    AdjacentRepetition,
    HookRepetition,
    WordFamilyEcho,
    PhraseEcho,
    RepeatedLine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepeatWarning {
    pub range: HighlightRange,
    pub kind: RepeatWarningKind,
    pub line_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StructureKind {
    Intro,
    Verse,
    Hook,
    Outro,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructureHighlight {
    pub range: HighlightRange,
    pub kind: StructureKind,
    pub bucket: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneHighlights {
    pub chains: Vec<HighlightRange>,
    pub repeats: Vec<RepeatHighlight>,
    pub warnings: Vec<RepeatWarning>,
    pub structures: Vec<StructureHighlight>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveHighlights {
    pub raw: PaneHighlights,
    pub final_: PaneHighlights,
}

#[derive(Clone, Debug)]
struct Token {
    normalized: String,
    stem: String,
    start: usize,
    end: usize,
    line_index: usize,
}

pub fn analyze(raw_text: &str, final_text: &str) -> LiveHighlights {
    LintEngine::default().analyze(raw_text, final_text)
}

#[derive(Clone, Debug, Default)]
pub struct LintEngine {
    raw_document: DocumentState,
    final_document: DocumentState,
    warnings_by_line: HashMap<usize, Vec<RepeatWarning>>,
}

impl LintEngine {
    pub fn analyze(mut self, raw_text: &str, final_text: &str) -> LiveHighlights {
        self.update(raw_text, final_text)
    }

    pub fn update(&mut self, raw_text: &str, final_text: &str) -> LiveHighlights {
        self.raw_document.update(raw_text);
        self.final_document.update(final_text);

        let final_words = self
            .final_document
            .tokens
            .iter()
            .map(|token| token.normalized.clone())
            .collect::<HashSet<_>>();

        let warnings = final_repeat_warnings(&self.final_document);
        self.warnings_by_line = warnings_by_line(&warnings);

        LiveHighlights {
            raw: PaneHighlights {
                chains: raw_used_word_ranges(&self.raw_document.tokens, &final_words),
                repeats: Vec::new(),
                warnings: Vec::new(),
                structures: Vec::new(),
            },
            final_: PaneHighlights {
                chains: Vec::new(),
                repeats: final_repeated_word_highlights(&self.final_document.tokens),
                warnings,
                structures: structure_highlights(final_text),
            },
        }
    }
}

#[derive(Clone, Debug, Default)]
struct DocumentState {
    text: String,
    lines: Vec<String>,
    tokens: Vec<Token>,
    tokens_by_line: Vec<Vec<usize>>,
    occurrences_by_token: HashMap<String, Vec<usize>>,
    occurrences_by_stem: HashMap<String, Vec<usize>>,
    phrase_index: HashMap<String, Vec<PhraseOccurrence>>,
    line_fingerprints: HashMap<String, Vec<LineOccurrence>>,
    section_by_line: Vec<Option<StructureKind>>,
}

impl DocumentState {
    fn update(&mut self, text: &str) {
        let new_lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
        let structure_changed = self.text != text && structure_tags_changed(&self.text, text);
        let changed_lines = changed_line_indexes(&self.lines, &new_lines);
        self.text = text.to_owned();
        self.lines = new_lines;

        if structure_changed || !changed_lines.is_empty() {
            self.rebuild_indexes();
        }
    }

    fn rebuild_indexes(&mut self) {
        self.tokens = tokenize(&self.text);
        self.tokens_by_line = vec![Vec::new(); self.lines.len().max(1)];
        self.occurrences_by_token.clear();
        self.occurrences_by_stem.clear();
        for (position, token) in self.tokens.iter().enumerate() {
            if let Some(line_tokens) = self.tokens_by_line.get_mut(token.line_index) {
                line_tokens.push(position);
            }
            self.occurrences_by_token
                .entry(token.normalized.clone())
                .or_default()
                .push(position);
            self.occurrences_by_stem
                .entry(token.stem.clone())
                .or_default()
                .push(position);
        }
        self.section_by_line = line_structure_map(&self.text);
        self.phrase_index = phrase_index(&self.tokens);
        self.line_fingerprints = line_fingerprints(&self.lines, &self.section_by_line);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PhraseOccurrence {
    phrase: String,
    range: HighlightRange,
    line_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LineOccurrence {
    fingerprint: String,
    line_index: usize,
}

fn warnings_by_line(warnings: &[RepeatWarning]) -> HashMap<usize, Vec<RepeatWarning>> {
    let mut by_line = HashMap::new();
    for warning in warnings {
        by_line
            .entry(warning.line_index)
            .or_insert_with(Vec::new)
            .push(warning.clone());
    }
    by_line
}

fn changed_line_indexes(old_lines: &[String], new_lines: &[String]) -> HashSet<usize> {
    let max_len = old_lines.len().max(new_lines.len());
    (0..max_len)
        .filter(|&index| old_lines.get(index) != new_lines.get(index))
        .collect()
}

fn structure_tags_changed(old_text: &str, new_text: &str) -> bool {
    structure_tags(old_text) != structure_tags(new_text)
}

fn is_ignored_stopword(word: &str) -> bool {
    IGNORED_STOPWORDS.contains(&word)
}

fn is_common_word(word: &str) -> bool {
    COMMON_WORDS.contains(&word)
}

fn is_weak_word(word: &str) -> bool {
    WEAK_ECHO_WORDS.contains(&word)
}

const IGNORED_STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "der", "die", "das", "den", "dem",
    "des", "du", "er", "es", "for", "from", "i", "ich", "im", "in", "is", "it", "me", "my", "of",
    "on", "or", "sie", "so", "the", "to", "und", "we", "wir", "you",
];

const COMMON_WORDS: &[&str] = &[
    "bin", "bist", "bleib", "bleibe", "geht", "gehen", "hab", "habe", "hat", "komm", "komme",
    "kommt", "leben", "liebe", "mach", "mache", "macht", "nacht", "sag", "sage", "sagt", "seh",
    "sehe", "tag", "weg", "welt", "zeit",
];

const WEAK_ECHO_WORDS: &[&str] = &[
    "auch",
    "eben",
    "einfach",
    "eigentlich",
    "irgendwie",
    "halt",
    "schon",
    "noch",
    "wieder",
    "wirklich",
    "ziemlich",
    "vielleicht",
    "quasi",
    "mal",
    "jetzt",
    "dann",
    "doch",
];

pub fn structure_sequence(text: &str) -> Vec<StructureHighlight> {
    structure_highlights(text)
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start = 0usize;
    let mut in_word = false;
    let mut line_index = 0usize;

    for (char_index, ch) in text.chars().enumerate() {
        if ch.is_alphanumeric() {
            if !in_word {
                start = char_index;
                in_word = true;
                current.clear();
            }
            current.push(ch);
        } else if in_word {
            push_token(&mut tokens, &current, start, char_index, line_index);
            in_word = false;
        }

        if ch == '\n' {
            line_index += 1;
        }
    }

    if in_word {
        push_token(
            &mut tokens,
            &current,
            start,
            text.chars().count(),
            line_index,
        );
    }

    tokens
}

fn push_token(tokens: &mut Vec<Token>, text: &str, start: usize, end: usize, line_index: usize) {
    let normalized = normalize_word(text);
    if normalized.is_empty() {
        return;
    }
    tokens.push(Token {
        stem: stem_word(&normalized),
        normalized,
        start,
        end,
        line_index,
    });
}

fn normalize_word(word: &str) -> String {
    word.to_uppercase().to_lowercase()
}

fn raw_used_word_ranges(tokens: &[Token], final_words: &HashSet<String>) -> Vec<HighlightRange> {
    let mut ranges = Vec::new();
    let mut active: Option<HighlightRange> = None;

    for token in tokens {
        if final_words.contains(&token.normalized) {
            match &mut active {
                Some(range) => range.end = token.end,
                None => {
                    active = Some(HighlightRange {
                        start: token.start,
                        end: token.end,
                    });
                }
            }
        } else if let Some(range) = active.take() {
            ranges.push(range);
        }
    }

    if let Some(range) = active {
        ranges.push(range);
    }

    ranges
}

fn final_repeated_word_highlights(tokens: &[Token]) -> Vec<RepeatHighlight> {
    let repeated = word_counts(tokens)
        .into_iter()
        .filter_map(|(word, count)| (count > 1).then_some(word))
        .collect::<HashSet<_>>();
    repeat_ranges_for_tokens(tokens, &repeated)
}

fn word_counts(tokens: &[Token]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for token in tokens {
        *counts.entry(token.normalized.clone()).or_insert(0) += 1;
    }
    counts
}

fn repeat_ranges_for_tokens(tokens: &[Token], repeated: &HashSet<String>) -> Vec<RepeatHighlight> {
    let mut positions_by_word: HashMap<&str, Vec<usize>> = HashMap::new();
    for (position, token) in tokens.iter().enumerate() {
        if repeated.contains(&token.normalized) {
            positions_by_word
                .entry(token.normalized.as_str())
                .or_default()
                .push(position);
        }
    }

    let mut highlights = Vec::new();
    for positions in positions_by_word.values() {
        for (local_index, &position) in positions.iter().enumerate() {
            let distance = nearest_distance(positions, local_index);
            let token = &tokens[position];
            highlights.push(RepeatHighlight {
                range: HighlightRange {
                    start: token.start,
                    end: token.end,
                },
                bucket: repeat_bucket(distance),
            });
        }
    }
    highlights.sort_by(|left, right| left.range.start.cmp(&right.range.start));
    highlights
}

fn final_repeat_warnings(document: &DocumentState) -> Vec<RepeatWarning> {
    let mut warnings = Vec::new();

    for positions in document
        .occurrences_by_token
        .values()
        .filter(|positions| positions.len() > 1)
    {
        let word = document.tokens[positions[0]].normalized.as_str();
        if is_ignored_stopword(word) {
            continue;
        }
        push_exact_word_warnings(document, positions, &mut warnings);
    }

    for positions in document
        .occurrences_by_stem
        .values()
        .filter(|positions| positions.len() > 1)
    {
        push_word_family_warnings(document, positions, &mut warnings);
    }

    for occurrences in document
        .phrase_index
        .values()
        .filter(|occurrences| occurrences.len() > 1)
    {
        push_phrase_warnings(occurrences, &mut warnings);
    }

    for occurrences in document
        .line_fingerprints
        .values()
        .filter(|occurrences| occurrences.len() > 1)
    {
        for occurrence in occurrences {
            if matches!(
                document.section_by_line.get(occurrence.line_index),
                Some(Some(StructureKind::Hook))
            ) {
                continue;
            }
            if let Some(range) = line_range(&document.text, occurrence.line_index) {
                warnings.push(RepeatWarning {
                    range,
                    kind: RepeatWarningKind::RepeatedLine,
                    line_index: occurrence.line_index,
                });
            }
        }
    }

    warnings.sort_by(|left, right| {
        left.line_index
            .cmp(&right.line_index)
            .then_with(|| {
                warning_severity(left.kind)
                    .cmp(&warning_severity(right.kind))
                    .reverse()
            })
            .then_with(|| left.range.start.cmp(&right.range.start))
    });
    warnings.dedup_by(|left, right| {
        left.kind == right.kind && left.line_index == right.line_index && left.range == right.range
    });
    warnings
}

fn push_exact_word_warnings(
    document: &DocumentState,
    positions: &[usize],
    warnings: &mut Vec<RepeatWarning>,
) {
    let word = document.tokens[positions[0]].normalized.as_str();
    let adjacent_positions = close_line_positions(&document.tokens, positions, 2);
    let hook_positions = positions
        .iter()
        .copied()
        .filter(|&position| {
            line_kind(document, document.tokens[position].line_index) == Some(StructureKind::Hook)
        })
        .collect::<HashSet<_>>();

    let scattered_weak = (is_weak_word(word) || is_common_word(word))
        && positions.len() >= 3
        && !all_positions_close(&document.tokens, positions, 2);

    for &position in positions {
        let token = &document.tokens[position];
        let kind = if hook_positions.contains(&position) {
            RepeatWarningKind::HookRepetition
        } else if adjacent_positions.contains(&position) {
            RepeatWarningKind::AdjacentRepetition
        } else if scattered_weak {
            RepeatWarningKind::ScatteredWeakWord
        } else {
            continue;
        };
        warnings.push(token_warning(token, kind));
    }
}

fn push_word_family_warnings(
    document: &DocumentState,
    positions: &[usize],
    warnings: &mut Vec<RepeatWarning>,
) {
    let unique_words = positions
        .iter()
        .map(|&position| document.tokens[position].normalized.as_str())
        .collect::<HashSet<_>>();
    if unique_words.len() < 2 {
        return;
    }
    for &position in positions {
        let token = &document.tokens[position];
        if is_ignored_stopword(&token.normalized) {
            continue;
        }
        warnings.push(token_warning(token, RepeatWarningKind::WordFamilyEcho));
    }
}

fn push_phrase_warnings(occurrences: &[PhraseOccurrence], warnings: &mut Vec<RepeatWarning>) {
    let unique_lines = occurrences
        .iter()
        .map(|occurrence| occurrence.line_index)
        .collect::<HashSet<_>>();
    if unique_lines.len() < 2 {
        return;
    }
    for occurrence in occurrences {
        warnings.push(RepeatWarning {
            range: occurrence.range.clone(),
            kind: RepeatWarningKind::PhraseEcho,
            line_index: occurrence.line_index,
        });
    }
}

fn token_warning(token: &Token, kind: RepeatWarningKind) -> RepeatWarning {
    RepeatWarning {
        range: HighlightRange {
            start: token.start,
            end: token.end,
        },
        kind,
        line_index: token.line_index,
    }
}

fn line_kind(document: &DocumentState, line_index: usize) -> Option<StructureKind> {
    document.section_by_line.get(line_index).copied().flatten()
}

fn warning_severity(kind: RepeatWarningKind) -> usize {
    match kind {
        RepeatWarningKind::ScatteredWeakWord => 6,
        RepeatWarningKind::AdjacentRepetition => 5,
        RepeatWarningKind::HookRepetition => 4,
        RepeatWarningKind::WordFamilyEcho => 3,
        RepeatWarningKind::PhraseEcho => 2,
        RepeatWarningKind::RepeatedLine => 1,
    }
}

fn close_line_positions(tokens: &[Token], positions: &[usize], max_gap: usize) -> HashSet<usize> {
    let mut marked = HashSet::new();
    for window in positions.windows(2) {
        let left = tokens[window[0]].line_index;
        let right = tokens[window[1]].line_index;
        if right.saturating_sub(left) <= max_gap {
            marked.insert(window[0]);
            marked.insert(window[1]);
        }
    }
    marked
}

fn all_positions_close(tokens: &[Token], positions: &[usize], max_gap: usize) -> bool {
    positions.windows(2).all(|window| {
        tokens[window[1]]
            .line_index
            .saturating_sub(tokens[window[0]].line_index)
            <= max_gap
    })
}

fn phrase_index(tokens: &[Token]) -> HashMap<String, Vec<PhraseOccurrence>> {
    let mut index = HashMap::new();
    for window_size in [2_usize, 3] {
        for window in tokens.windows(window_size) {
            if window
                .iter()
                .any(|token| is_ignored_stopword(&token.normalized))
            {
                continue;
            }
            let first = &window[0];
            let last = &window[window.len() - 1];
            if first.line_index != last.line_index {
                continue;
            }
            let phrase = window
                .iter()
                .map(|token| token.normalized.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            index
                .entry(phrase.clone())
                .or_insert_with(Vec::new)
                .push(PhraseOccurrence {
                    phrase,
                    range: HighlightRange {
                        start: first.start,
                        end: last.end,
                    },
                    line_index: first.line_index,
                });
        }
    }
    index
}

fn line_fingerprints(
    lines: &[String],
    section_by_line: &[Option<StructureKind>],
) -> HashMap<String, Vec<LineOccurrence>> {
    let mut fingerprints = HashMap::new();
    for (line_index, line) in lines.iter().enumerate() {
        if matches!(
            section_by_line.get(line_index),
            Some(Some(StructureKind::Hook))
        ) {
            continue;
        }
        let fingerprint = tokenize(line)
            .into_iter()
            .map(|token| token.normalized)
            .filter(|word| !is_ignored_stopword(word))
            .collect::<Vec<_>>()
            .join(" ");
        if fingerprint.split_whitespace().count() < 3 {
            continue;
        }
        fingerprints
            .entry(fingerprint.clone())
            .or_insert_with(Vec::new)
            .push(LineOccurrence {
                fingerprint,
                line_index,
            });
    }
    fingerprints
}

fn line_range(text: &str, target_line: usize) -> Option<HighlightRange> {
    let mut start = 0usize;
    for (line_index, line) in text.split_inclusive('\n').enumerate() {
        let end = start + line.trim_end_matches('\n').chars().count();
        if line_index == target_line {
            return Some(HighlightRange { start, end });
        }
        start += line.chars().count();
    }
    (target_line == text.lines().count()).then_some(HighlightRange { start, end: start })
}

fn stem_word(word: &str) -> String {
    for suffix in [
        "ungen", "keit", "lich", "isch", "ern", "en", "er", "es", "e", "s",
    ] {
        if word.chars().count() > suffix.chars().count() + 3 && word.ends_with(suffix) {
            return word
                .chars()
                .take(word.chars().count() - suffix.chars().count())
                .collect();
        }
    }
    word.to_owned()
}

fn nearest_distance(positions: &[usize], local_index: usize) -> Option<usize> {
    if positions.len() < 2 {
        return None;
    }

    let current = positions[local_index];
    let previous = local_index
        .checked_sub(1)
        .map(|index| current.saturating_sub(positions[index]));
    let next = positions
        .get(local_index + 1)
        .map(|position| position.saturating_sub(current));

    match (previous, next) {
        (Some(previous), Some(next)) => Some(previous.min(next)),
        (Some(previous), None) => Some(previous),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}

fn repeat_bucket(distance: Option<usize>) -> usize {
    match distance {
        Some(1..=2) => 5,
        Some(3..=5) => 4,
        Some(6..=10) => 3,
        Some(11..=20) => 2,
        Some(21..=40) => 1,
        Some(_) | None => 0,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StructureTag {
    kind: StructureKind,
    start: usize,
}

fn line_structure_map(text: &str) -> Vec<Option<StructureKind>> {
    let tags = structure_tags(text);
    let mut line_offsets = vec![0];
    for (index, ch) in text.chars().enumerate() {
        if ch == '\n' {
            line_offsets.push(index + 1);
        }
    }

    let mut result = Vec::with_capacity(line_offsets.len());
    let mut current_kind = None;
    let mut next_tag_index = 0;
    for start in line_offsets {
        while let Some(tag) = tags.get(next_tag_index) {
            if tag.start <= start {
                current_kind = Some(tag.kind);
                next_tag_index += 1;
            } else {
                break;
            }
        }
        result.push(current_kind);
    }
    result
}

fn structure_highlights(text: &str) -> Vec<StructureHighlight> {
    let tags = structure_tags(text);
    if tags.is_empty() {
        return Vec::new();
    }

    let mut totals = HashMap::<StructureKind, usize>::new();
    for tag in &tags {
        *totals.entry(tag.kind).or_default() += 1;
    }

    let mut seen = HashMap::<StructureKind, usize>::new();
    let text_end = text.chars().count();
    tags.iter()
        .enumerate()
        .filter_map(|(index, tag)| {
            let end = tags
                .get(index + 1)
                .map(|next| next.start)
                .unwrap_or(text_end);
            if tag.start >= end {
                return None;
            }

            let occurrence_index = seen.entry(tag.kind).or_default();
            let bucket = structure_bucket(
                tag.kind,
                *occurrence_index,
                totals.get(&tag.kind).copied().unwrap_or(1),
            );
            *occurrence_index += 1;

            Some(StructureHighlight {
                range: HighlightRange {
                    start: tag.start,
                    end,
                },
                kind: tag.kind,
                bucket,
            })
        })
        .collect()
}

fn structure_tags(text: &str) -> Vec<StructureTag> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut tags = Vec::new();
    let mut offset = 0usize;

    while offset < chars.len() {
        if chars[offset] != '[' {
            offset += 1;
            continue;
        }

        let Some(close_offset) = chars[offset + 1..]
            .iter()
            .position(|ch| *ch == ']')
            .map(|position| offset + 1 + position)
        else {
            offset += 1;
            continue;
        };

        let label = chars[offset + 1..close_offset].iter().collect::<String>();
        if let Some(kind) = parse_structure_kind(&label) {
            tags.push(StructureTag {
                kind,
                start: offset,
            });
        }
        offset = close_offset + 1;
    }

    tags
}

fn parse_structure_kind(label: &str) -> Option<StructureKind> {
    let normalized = label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    match normalized.as_str() {
        "intro" => Some(StructureKind::Intro),
        "hook" | "chorus" => Some(StructureKind::Hook),
        "outro" => Some(StructureKind::Outro),
        _ => parse_numbered_structure(&normalized, "verse", StructureKind::Verse)
            .or_else(|| parse_numbered_structure(&normalized, "hook", StructureKind::Hook)),
    }
}

fn parse_numbered_structure(
    label: &str,
    prefix: &str,
    kind: StructureKind,
) -> Option<StructureKind> {
    let number = label.strip_prefix(&format!("{prefix} "))?;
    let parsed = number.parse::<u8>().ok()?;
    (1..=99).contains(&parsed).then_some(kind)
}

fn structure_bucket(kind: StructureKind, occurrence_index: usize, total: usize) -> usize {
    match kind {
        StructureKind::Verse | StructureKind::Hook if total > 1 => {
            occurrence_index.saturating_mul(STRUCTURE_BUCKETS - 1) / (total - 1)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_used_words_are_marked_as_raw_chains_only() {
        let highlights = analyze("eins zwei drei", "null EINS ZWEI ende");
        assert_eq!(
            highlights.raw.chains,
            vec![HighlightRange { start: 0, end: 9 }]
        );
        assert!(highlights.final_.chains.is_empty());
        assert!(highlights.raw.repeats.is_empty());
    }

    #[test]
    fn single_final_used_words_update_raw_pane() {
        let highlights = analyze("eins zwei", "eins ende");
        assert_eq!(
            highlights.raw.chains,
            vec![HighlightRange { start: 0, end: 4 }]
        );
        assert!(highlights.final_.chains.is_empty());
    }

    #[test]
    fn duplicate_words_are_marked_red_in_final_pane_only() {
        let highlights = analyze("hook alpha hook", "HOOK outro hook");
        assert!(highlights.raw.repeats.is_empty());
        assert_eq!(highlights.final_.repeats.len(), 2);
    }

    #[test]
    fn repeat_intensity_is_stronger_when_words_are_close() {
        let close = analyze("", "hook hook");
        let far = analyze(
            "",
            "hook eins zwei drei vier fuenf sechs sieben acht neun zehn elf hook",
        );

        assert!(close.final_.repeats[0].bucket > far.final_.repeats[0].bucket);
    }

    #[test]
    fn repeated_words_are_case_independent() {
        let highlights = analyze("nichts", "Straße pause STRASSE");
        assert!(highlights.raw.repeats.is_empty());
        assert_eq!(highlights.final_.repeats.len(), 2);
    }

    #[test]
    fn raw_pane_drops_used_word_mark_when_final_word_is_removed() {
        let with_word = analyze("eins zwei", "zwei");
        let without_word = analyze("eins zwei", "");
        assert_eq!(
            with_word.raw.chains,
            vec![HighlightRange { start: 5, end: 9 }]
        );
        assert!(without_word.raw.chains.is_empty());
    }

    #[test]
    fn structure_tags_color_final_sections_until_next_tag() {
        let highlights = analyze(
            "",
            "[intro]\nopen\n[verse 1]\nfirst\n[hook]\nrepeat\n[outro]\nclose",
        );

        assert_eq!(highlights.final_.structures.len(), 4);
        assert_eq!(highlights.final_.structures[0].kind, StructureKind::Intro);
        assert_eq!(highlights.final_.structures[1].kind, StructureKind::Verse);
        assert_eq!(highlights.final_.structures[2].kind, StructureKind::Hook);
        assert_eq!(highlights.final_.structures[3].kind, StructureKind::Outro);
        assert_eq!(highlights.final_.structures[0].range.start, 0);
        assert_eq!(
            highlights.final_.structures[0].range.end,
            "[intro]\nopen\n".chars().count()
        );
        assert_eq!(
            highlights.final_.structures[3].range.end,
            "[intro]\nopen\n[verse 1]\nfirst\n[hook]\nrepeat\n[outro]\nclose"
                .chars()
                .count()
        );
    }

    #[test]
    fn structure_parser_accepts_verse_one_to_ninety_nine_only() {
        let highlights = analyze(
            "",
            "[verse 1]\na\n[verse 99]\nb\n[verse 100]\nc\n[verse 0]\nd",
        );

        assert_eq!(highlights.final_.structures.len(), 2);
        assert!(
            highlights
                .final_
                .structures
                .iter()
                .all(|section| section.kind == StructureKind::Verse)
        );
    }

    #[test]
    fn structure_parser_accepts_numbered_hooks_for_structure_tool() {
        let highlights = analyze("", "[hook 1]\na\n[HOOK 2]\nb\n[hook 100]\nignored");

        assert_eq!(highlights.final_.structures.len(), 2);
        assert!(
            highlights
                .final_
                .structures
                .iter()
                .all(|section| section.kind == StructureKind::Hook)
        );
    }

    #[test]
    fn verse_and_hook_structure_buckets_brighten_by_occurrence() {
        let sections =
            structure_sequence("[verse 1]\na\n[hook]\nh\n[verse 2]\nb\n[hook]\nh\n[verse 3]\nc");
        let verse_buckets = sections
            .iter()
            .filter_map(|section| (section.kind == StructureKind::Verse).then_some(section.bucket))
            .collect::<Vec<_>>();
        let hook_buckets = sections
            .iter()
            .filter_map(|section| (section.kind == StructureKind::Hook).then_some(section.bucket))
            .collect::<Vec<_>>();

        assert_eq!(verse_buckets, vec![0, 2, 5]);
        assert_eq!(hook_buckets, vec![0, 5]);
    }

    #[test]
    fn raw_pane_does_not_receive_structure_highlights() {
        let highlights = analyze("[intro]\nraw", "[intro]\nfinal");

        assert!(highlights.raw.structures.is_empty());
        assert_eq!(highlights.final_.structures.len(), 1);
    }

    #[test]
    fn scattered_weak_words_get_scattered_warning() {
        let highlights = analyze(
            "",
            "[VERSE 1]\ndoch\nline two\nline three\nnoch doch\nline five\nline six\ndoch",
        );
        let scattered = highlights
            .final_
            .warnings
            .iter()
            .filter(|warning| warning.kind == RepeatWarningKind::ScatteredWeakWord)
            .collect::<Vec<_>>();

        assert_eq!(scattered.len(), 3);
    }

    #[test]
    fn neighboring_repeated_words_get_adjacent_repetition_warning() {
        let highlights = analyze("", "chorus\nCHORUS");
        let adjacent = highlights
            .final_
            .warnings
            .iter()
            .filter(|warning| warning.kind == RepeatWarningKind::AdjacentRepetition)
            .count();

        assert_eq!(adjacent, 2);
    }

    #[test]
    fn hook_repetition_gets_hook_warning() {
        let highlights = analyze("", "[HOOK]\nchorus\nCHORUS");
        let hook = highlights
            .final_
            .warnings
            .iter()
            .filter(|warning| warning.kind == RepeatWarningKind::HookRepetition)
            .count();

        assert_eq!(hook, 2);
    }
}
