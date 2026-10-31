//! The language of the messages the launcher writes: Simplified Chinese or
//! English, following the interface. Messages are written as pairs with
//! `tr!`, so each stays next to its translation.
use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_english(on: bool) {
    ENGLISH.store(on, Ordering::Relaxed);
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// `tr!("中文 {x}", "English {x}", args…)`: the message in the current
/// language, formatted like `format!`.
#[macro_export]
macro_rules! tr {
    ($zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::core::i18n::english() { format!($en $(, $arg)*) } else { format!($zh $(, $arg)*) }
    };
}

/// For `&'static str` places: picks one of two fixed strings.
pub fn pick(zh: &'static str, en: &'static str) -> &'static str {
    if english() { en } else { zh }
}

#[cfg(test)]
mod tests {
    // Tests run in parallel and others check Chinese messages, so this one
    // leaves the shared language alone and only checks the default.
    #[test]
    fn messages_are_chinese_by_default_and_format_their_arguments() {
        let n = 2;
        assert!(!super::english());
        assert_eq!(crate::tr!("共 {n} 个", "{n} in all"), "共 2 个");
        assert_eq!(crate::tr!("第 {} 个", "number {}", n + 1), "第 3 个");
        assert_eq!(super::pick("模组", "mod"), "模组");
    }
}
