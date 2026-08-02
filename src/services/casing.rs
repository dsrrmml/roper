use crate::models::CasingMode;

pub fn apply_casing(text: &str, mode: CasingMode) -> String {
    match mode {
        CasingMode::Preserve => text.to_owned(),
        CasingMode::Uppercase => text.to_uppercase(),
        CasingMode::Lowercase => text.to_lowercase(),
    }
}

pub fn needs_casing(text: &str, mode: CasingMode) -> bool {
    match mode {
        CasingMode::Preserve => false,
        CasingMode::Uppercase => text != text.to_uppercase(),
        CasingMode::Lowercase => text != text.to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casing_preserve_keeps_text() {
        assert_eq!(apply_casing("AbC äÖ", CasingMode::Preserve), "AbC äÖ");
    }

    #[test]
    fn casing_uppercase_is_unicode_aware() {
        assert_eq!(
            apply_casing("straße café", CasingMode::Uppercase),
            "STRASSE CAFÉ"
        );
    }

    #[test]
    fn casing_lowercase_is_unicode_aware() {
        assert_eq!(
            apply_casing("İSTANBUL ÖL", CasingMode::Lowercase),
            "i̇stanbul öl"
        );
    }
}
