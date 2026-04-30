pub fn estimate_briefing_tokens(text: &str) -> u32 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }
    ((trimmed.chars().count() as u32) + 3) / 4
}

pub fn text_within_token_budget(text: &str, max_tokens: usize) -> bool {
    text.chars().count() <= max_tokens.saturating_mul(4)
}
