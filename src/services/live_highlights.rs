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
    Skull,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepeatWarning {
    pub range: HighlightRange,
    pub kind: RepeatWarningKind,
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
    start: usize,
    end: usize,
    line_index: usize,
}

pub fn analyze(raw_text: &str, final_text: &str) -> LiveHighlights {
    let raw_tokens = tokenize(raw_text);
    let final_tokens = tokenize(final_text);
    let final_words = final_tokens
        .iter()
        .map(|token| token.normalized.clone())
        .collect::<HashSet<_>>();

    LiveHighlights {
        raw: PaneHighlights {
            chains: raw_used_word_ranges(&raw_tokens, &final_words),
            repeats: Vec::new(),
            warnings: Vec::new(),
            structures: Vec::new(),
        },
        final_: PaneHighlights {
            chains: Vec::new(),
            repeats: final_repeated_word_highlights(&final_tokens),
            warnings: final_repeat_warnings(&final_tokens),
            structures: structure_highlights(final_text),
        },
    }
}

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

fn final_repeat_warnings(tokens: &[Token]) -> Vec<RepeatWarning> {
    let mut positions_by_word: HashMap<&str, Vec<usize>> = HashMap::new();
    for (position, token) in tokens.iter().enumerate() {
        positions_by_word
            .entry(token.normalized.as_str())
            .or_default()
            .push(position);
    }

    let mut warnings = Vec::new();
    for positions in positions_by_word
        .values()
        .filter(|positions| positions.len() > 1)
    {
        let skull_positions = skull_warning_positions(tokens, positions);
        let adjacent_positions = adjacent_line_warning_positions(tokens, positions);

        for &position in positions {
            let kind = if skull_positions.contains(&position) {
                Some(RepeatWarningKind::Skull)
            } else if adjacent_positions.contains(&position) {
                Some(RepeatWarningKind::Warning)
            } else {
                None
            };
            let Some(kind) = kind else {
                continue;
            };
            let token = &tokens[position];
            warnings.push(RepeatWarning {
                range: HighlightRange {
                    start: token.start,
                    end: token.end,
                },
                kind,
            });
        }
    }

    warnings.sort_by(|left, right| left.range.start.cmp(&right.range.start));
    warnings
}

fn skull_warning_positions(tokens: &[Token], positions: &[usize]) -> HashSet<usize> {
    let mut marked = HashSet::new();
    if positions
        .first()
        .is_none_or(|position| tokens[*position].normalized.chars().count() < 5)
    {
        return marked;
    }

    for start in 0..positions.len() {
        for end in (start + 2)..positions.len() {
            let start_line = tokens[positions[start]].line_index;
            let end_line = tokens[positions[end]].line_index;
            if end_line.saturating_sub(start_line) > 3 {
                break;
            }
            for &position in &positions[start..=end] {
                marked.insert(position);
            }
        }
    }
    marked
}

fn adjacent_line_warning_positions(tokens: &[Token], positions: &[usize]) -> HashSet<usize> {
    let mut marked = HashSet::new();
    for (local_index, &position) in positions.iter().enumerate() {
        let line_index = tokens[position].line_index;
        if positions.iter().enumerate().any(|(other_index, &other)| {
            other_index != local_index && tokens[other].line_index.abs_diff(line_index) == 1
        }) {
            marked.insert(position);
        }
    }
    marked
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
        "hook" => Some(StructureKind::Hook),
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
    fn long_word_used_more_than_twice_within_four_lines_gets_skulls() {
        let highlights = analyze("", "dangerous\none dangerous\ntwo dangerous");
        assert_eq!(highlights.final_.warnings.len(), 3);
        assert!(
            highlights
                .final_
                .warnings
                .iter()
                .all(|warning| warning.kind == RepeatWarningKind::Skull)
        );
    }

    #[test]
    fn five_character_word_used_more_than_twice_within_four_lines_gets_skulls() {
        let highlights = analyze("", "spast\nso ein spast\nso ein spast");
        let spast_warnings = highlights
            .final_
            .warnings
            .iter()
            .filter(|warning| warning.range.end - warning.range.start == 5)
            .collect::<Vec<_>>();

        assert_eq!(spast_warnings.len(), 3);
        assert!(
            spast_warnings
                .iter()
                .all(|warning| warning.kind == RepeatWarningKind::Skull)
        );
    }

    #[test]
    fn two_consecutive_line_uses_get_warning_polygons() {
        let highlights = analyze("", "chorus\nCHORUS");
        assert_eq!(highlights.final_.warnings.len(), 2);
        assert!(
            highlights
                .final_
                .warnings
                .iter()
                .all(|warning| warning.kind == RepeatWarningKind::Warning)
        );
    }

    #[test]
    fn skull_warning_takes_precedence_over_consecutive_line_warning() {
        let highlights = analyze("", "dangerous\nDANGEROUS\ndangerous");
        assert_eq!(highlights.final_.warnings.len(), 3);
        assert!(
            highlights
                .final_
                .warnings
                .iter()
                .all(|warning| warning.kind == RepeatWarningKind::Skull)
        );
    }
}
