//! Rough token counts for budgeting: no tokenizer ships with the app. CJK characters count
//! as one token each, anything else as one per three characters. Both err on the high side
//! for common tokenizers, so a budget is rarely exceeded.

pub fn estimate(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if is_cjk(ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(3)
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3000..=0x303f   // CJK punctuation
        | 0x3040..=0x30ff // kana
        | 0x3400..=0x4dbf
        | 0x4e00..=0x9fff
        | 0xac00..=0xd7af // Hangul
        | 0xf900..=0xfaff
        | 0xff00..=0xffef // full-width forms
        | 0x20000..=0x2ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_counts_per_character_and_latin_per_three() {
        assert_eq!(estimate(""), 0);
        assert_eq!(estimate("我们开会"), 4);
        assert_eq!(estimate("hello world"), 4);
        // Four CJK characters (with "。"), twelve others.
        assert_eq!(estimate("用 Kubernetes 部署。"), 4 + 4);
    }
}
