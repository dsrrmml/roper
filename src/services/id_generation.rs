use crate::error_handling::{AppError, AppResult};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn md5_hex(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}

pub fn id_from_seed(seed: &str) -> String {
    md5_hex(seed).chars().take(12).collect()
}

pub fn generate_id(context: &str) -> String {
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let seed = format!("{}:{}:{}:{}", context, std::process::id(), now, counter);
    id_from_seed(&seed)
}

pub fn generate_unique_id<F>(context: &str, mut is_taken: F) -> AppResult<String>
where
    F: FnMut(&str) -> bool,
{
    for attempt in 0..2048_u32 {
        let seed = format!("{}:{}", context, attempt);
        let candidate = if attempt == 0 {
            generate_id(context)
        } else {
            generate_id(&seed)
        };
        if !is_taken(&candidate) {
            return Ok(candidate);
        }
    }

    Err(AppError::validation(
        "id",
        "could not generate a collision-free id",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::validation::is_valid_id;
    use std::collections::HashSet;

    #[test]
    fn generated_id_is_valid_twelve_hex_chars() {
        let id = generate_id("test");
        assert!(is_valid_id(&id));
    }

    #[test]
    fn collision_handling_retries_until_free() {
        let mut seen = HashSet::new();
        let id = generate_unique_id("collision", |candidate| {
            if seen.is_empty() {
                seen.insert(candidate.to_owned());
                true
            } else {
                false
            }
        })
        .expect("test id generation should find a free candidate");

        assert!(is_valid_id(&id));
        assert_eq!(seen.len(), 1);
    }
}
