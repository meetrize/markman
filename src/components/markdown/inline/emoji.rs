//! GitHub-style `:shortcode:` emoji expansion.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::fragment::InlineEmoji;
use super::normalize::{CharToken, NormalizeBuilder};
use super::style::InlineStyle;
use crate::components::markdown::html::HtmlInlineStyle;

static EMOJI_SHORTCODES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("+1", "👍"),
        ("-1", "👎"),
        ("100", "💯"),
        ("1234", "🔢"),
        ("smile", "😄"),
        ("grin", "😁"),
        ("joy", "😂"),
        ("rofl", "🤣"),
        ("wink", "😉"),
        ("blush", "😊"),
        ("heart", "❤️"),
        ("heart_eyes", "😍"),
        ("kissing_heart", "😘"),
        ("thinking", "🤔"),
        ("neutral_face", "😐"),
        ("unamused", "😒"),
        ("sweat_smile", "😅"),
        ("sob", "😭"),
        ("angry", "😠"),
        ("rage", "😡"),
        ("confused", "😕"),
        ("astonished", "😲"),
        ("scream", "😱"),
        ("cry", "😢"),
        ("disappointed", "😞"),
        ("sweat", "😓"),
        ("weary", "😩"),
        ("ok_hand", "👌"),
        ("clap", "👏"),
        ("pray", "🙏"),
        ("muscle", "💪"),
        ("wave", "👋"),
        ("thumbsup", "👍"),
        ("thumbsdown", "👎"),
        ("point_up", "☝️"),
        ("point_down", "👇"),
        ("point_left", "👈"),
        ("point_right", "👉"),
        ("raised_hands", "🙌"),
        ("rocket", "🚀"),
        ("fire", "🔥"),
        ("star", "⭐"),
        ("sparkles", "✨"),
        ("tada", "🎉"),
        ("party_popper", "🎉"),
        ("warning", "⚠️"),
        ("exclamation", "❗"),
        ("question", "❓"),
        ("white_check_mark", "✅"),
        ("check", "✅"),
        ("x", "❌"),
        ("no_entry", "⛔"),
        ("bulb", "💡"),
        ("memo", "📝"),
        ("pencil", "✏️"),
        ("book", "📖"),
        ("books", "📚"),
        ("computer", "💻"),
        ("phone", "📱"),
        ("email", "📧"),
        ("link", "🔗"),
        ("globe_with_meridians", "🌐"),
        ("earth_asia", "🌏"),
        ("sunny", "☀️"),
        ("cloud", "☁️"),
        ("umbrella", "☔"),
        ("snowflake", "❄️"),
        ("zap", "⚡"),
        ("bug", "🐛"),
        ("bee", "🐝"),
        ("dog", "🐶"),
        ("cat", "🐱"),
        ("panda_face", "🐼"),
        ("coffee", "☕"),
        ("beer", "🍺"),
        ("pizza", "🍕"),
        ("cake", "🎂"),
        ("gift", "🎁"),
        ("trophy", "🏆"),
        ("medal", "🏅"),
        ("soccer", "⚽"),
        ("basketball", "🏀"),
        ("football", "🏈"),
        ("baseball", "⚾"),
        ("tennis", "🎾"),
        ("car", "🚗"),
        ("bus", "🚌"),
        ("airplane", "✈️"),
        ("ship", "🚢"),
        ("house", "🏠"),
        ("office", "🏢"),
        ("hospital", "🏥"),
        ("school", "🏫"),
        ("clock", "🕐"),
        ("hourglass", "⌛"),
        ("eyes", "👀"),
        ("see_no_evil", "🙈"),
        ("hear_no_evil", "🙉"),
        ("speak_no_evil", "🙊"),
        ("skull", "💀"),
        ("ghost", "👻"),
        ("alien", "👽"),
        ("robot", "🤖"),
        ("poop", "💩"),
        ("hankey", "💩"),
        ("cn", "🇨🇳"),
        ("us", "🇺🇸"),
        ("jp", "🇯🇵"),
        ("kr", "🇰🇷"),
        ("gb", "🇬🇧"),
        ("uk", "🇬🇧"),
        ("de", "🇩🇪"),
        ("fr", "🇫🇷"),
        ("es", "🇪🇸"),
        ("it", "🇮🇹"),
        ("ru", "🇷🇺"),
        ("in", "🇮🇳"),
        ("br", "🇧🇷"),
        ("ca", "🇨🇦"),
        ("au", "🇦🇺"),
    ])
});

/// Resolves a shortcode name (without colons) to its emoji glyph.
pub(crate) fn resolve_emoji_shortcode(name: &str) -> Option<String> {
    let normalized = name.to_ascii_lowercase();
    if let Some(glyph) = EMOJI_SHORTCODES.get(normalized.as_str()) {
        return Some((*glyph).to_string());
    }
    flag_emoji_from_country_code(&normalized)
}

fn flag_emoji_from_country_code(code: &str) -> Option<String> {
    if code.len() != 2 || !code.is_ascii() {
        return None;
    }
    let upper = code.to_ascii_uppercase();
    let mut chars = String::new();
    for byte in upper.bytes() {
        if !byte.is_ascii_alphabetic() {
            return None;
        }
        chars.push(char::from_u32(0x1F1E6 + (byte - b'A') as u32)?);
    }
    Some(chars)
}

pub(crate) fn parse_emoji_shortcode(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
) -> Option<usize> {
    if tokens.get(index)?.ch != ':' {
        return None;
    }

    let mut cursor = index + 1;
    let mut name = String::new();
    while cursor < tokens.len() {
        let ch = tokens[cursor].ch;
        if ch == ':' {
            if name.is_empty() {
                return None;
            }
            let source = format!(":{name}:");
            let glyph = resolve_emoji_shortcode(&name)?;
            emit_emoji_fragment(
                tokens,
                index,
                cursor,
                &source,
                &glyph,
                extra_style,
                extra_html_style,
                builder,
            );
            return Some(cursor + 1);
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '_') {
            name.push(ch);
            cursor += 1;
            continue;
        }
        return None;
    }

    None
}

fn emit_emoji_fragment(
    tokens: &[CharToken],
    start_index: usize,
    end_index: usize,
    source: &str,
    glyph: &str,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
) {
    let normalized_start = builder.normalized_len;
    let visible_len = glyph.len();
    let normalized_end = normalized_start + visible_len;

    for token in &tokens[start_index..=end_index] {
        let token_len = token.source_range.len();
        for delta in 0..=token_len {
            builder.visible_to_normalized[token.source_range.start + delta] = normalized_start;
        }
    }
    for boundary in tokens[end_index].source_range.end..=tokens[end_index].source_range.end {
        builder.visible_to_normalized[boundary] = normalized_end;
    }

    builder.normalized_len += visible_len;
    builder.fragments.push(super::fragment::InlineFragment {
        text: glyph.to_string(),
        style: extra_style,
        html_style: extra_html_style,
        link: None,
        footnote: None,
        math: None,
        emoji: Some(InlineEmoji {
            source: source.to_string(),
            glyph: glyph.to_string(),
        }),
        tag: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_shortcodes() {
        assert_eq!(resolve_emoji_shortcode("smile"), Some("😄".into()));
        assert_eq!(resolve_emoji_shortcode("+1"), Some("👍".into()));
        assert_eq!(resolve_emoji_shortcode("rocket"), Some("🚀".into()));
        assert_eq!(resolve_emoji_shortcode("cn"), Some("🇨🇳".into()));
        assert_eq!(resolve_emoji_shortcode("warning"), Some("⚠️".into()));
    }

    #[test]
    fn rejects_unknown_shortcodes() {
        assert!(resolve_emoji_shortcode("not_a_real_emoji_name").is_none());
        assert!(resolve_emoji_shortcode("abc").is_none());
    }
}
