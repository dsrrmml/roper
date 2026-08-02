use crate::models::{CasingMode, UsedMaterial};
use crate::services::id_generation::md5_hex;
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawLineIdentity {
    pub line_index: usize,
    pub text: String,
    pub normalized: String,
    pub normalized_hash: String,
    pub occurrence: usize,
}

pub fn normalize_line(line: &str, _casing_mode: CasingMode) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut normalized = String::new();
    let mut in_horizontal_space = false;
    for ch in trimmed.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        if ch.is_whitespace() {
            if !in_horizontal_space {
                normalized.push(' ');
                in_horizontal_space = true;
            }
        } else {
            normalized.push(ch);
            in_horizontal_space = false;
        }
    }

    let normalized = normalized.to_uppercase().to_lowercase();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub fn raw_line_identities(raw: &str, casing_mode: CasingMode) -> Vec<RawLineIdentity> {
    let mut occurrence_by_hash: HashMap<String, usize> = HashMap::new();
    raw.lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let normalized = normalize_line(line, casing_mode)?;
            let normalized_hash = md5_hex(&normalized);
            let occurrence = occurrence_by_hash
                .entry(normalized_hash.clone())
                .and_modify(|value| *value += 1)
                .or_insert(0);
            Some(RawLineIdentity {
                line_index,
                text: line.to_owned(),
                normalized,
                normalized_hash,
                occurrence: *occurrence,
            })
        })
        .collect()
}

pub fn material_from_identity(identity: &RawLineIdentity) -> UsedMaterial {
    UsedMaterial {
        normalized_hash: identity.normalized_hash.clone(),
        occurrence: identity.occurrence,
    }
}

pub fn contains_material(entries: &[UsedMaterial], candidate: &UsedMaterial) -> bool {
    entries.iter().any(|entry| {
        entry.normalized_hash == candidate.normalized_hash
            && entry.occurrence == candidate.occurrence
    })
}

pub fn add_used_material(entries: &mut Vec<UsedMaterial>, candidate: UsedMaterial) -> bool {
    if contains_material(entries, &candidate) {
        false
    } else {
        entries.push(candidate);
        true
    }
}

pub fn effective_used_material(
    raw: &str,
    _final_text: &str,
    casing_mode: CasingMode,
    manual_entries: &[UsedMaterial],
    _dismissed_entries: &[UsedMaterial],
) -> Vec<UsedMaterial> {
    let raw_entries = raw_line_identities(raw, casing_mode)
        .iter()
        .map(material_from_identity)
        .collect::<Vec<_>>();
    manual_entries
        .iter()
        .filter(|entry| contains_material(&raw_entries, entry))
        .cloned()
        .collect()
}

pub fn remove_used_material(entries: &mut Vec<UsedMaterial>, candidate: &UsedMaterial) -> bool {
    let before = entries.len();
    entries.retain(|entry| {
        !(entry.normalized_hash == candidate.normalized_hash
            && entry.occurrence == candidate.occurrence)
    });
    entries.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_normalization_collapses_horizontal_space() {
        assert_eq!(
            normalize_line("  eins\t  zwei  ", CasingMode::Preserve),
            Some("eins zwei".to_owned())
        );
    }

    #[test]
    fn identical_lines_get_distinct_occurrences() {
        let identities = raw_line_identities("A\nA\nB\nA", CasingMode::Lowercase);
        let occurrences: Vec<usize> = identities
            .iter()
            .filter(|identity| identity.normalized == "a")
            .map(|identity| identity.occurrence)
            .collect();
        assert_eq!(occurrences, vec![0, 1, 2]);
    }

    #[test]
    fn material_identity_does_not_depend_on_casing_mode() {
        let preserve = raw_line_identities("Straße", CasingMode::Preserve)
            .into_iter()
            .next()
            .map(|identity| material_from_identity(&identity));
        let uppercase = raw_line_identities("Straße", CasingMode::Uppercase)
            .into_iter()
            .next()
            .map(|identity| material_from_identity(&identity));
        let lowercase = raw_line_identities("Straße", CasingMode::Lowercase)
            .into_iter()
            .next()
            .map(|identity| material_from_identity(&identity));
        assert_eq!(preserve, uppercase);
        assert_eq!(uppercase, lowercase);
    }

    #[test]
    fn duplicate_material_tracking_is_explicit() {
        let identities = raw_line_identities("A\nA", CasingMode::Preserve);
        let manual = vec![material_from_identity(&identities[1])];
        let used = effective_used_material("A\nA", "A\nA", CasingMode::Uppercase, &manual, &[]);
        assert_eq!(used, manual);
    }

    #[test]
    fn final_text_does_not_create_used_material() {
        let used = effective_used_material(
            "first\nsecond",
            "FIRST\nSECOND",
            CasingMode::Uppercase,
            &[],
            &[],
        );
        assert!(used.is_empty());
    }

    #[test]
    fn persistent_material_entries_can_be_removed() {
        let identity = raw_line_identities("A", CasingMode::Preserve)
            .into_iter()
            .next()
            .expect("test identity exists");
        let entry = material_from_identity(&identity);
        let mut entries = Vec::new();
        assert!(add_used_material(&mut entries, entry.clone()));
        assert!(!add_used_material(&mut entries, entry.clone()));
        assert!(remove_used_material(&mut entries, &entry));
        assert!(entries.is_empty());
    }

    #[test]
    fn stale_manual_material_is_not_effective() {
        let stale = UsedMaterial {
            normalized_hash: "missing".to_owned(),
            occurrence: 0,
        };
        let used = effective_used_material("first", "first", CasingMode::Preserve, &[stale], &[]);
        assert!(used.is_empty());
    }
}
