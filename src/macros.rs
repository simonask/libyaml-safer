macro_rules! CHECK_AT {
    ($buffer:expr, $octet:expr, $offset:expr) => {
        $buffer.get($offset).copied() == Some($octet)
    };
}

macro_rules! CHECK {
    ($buffer:expr, $octet:expr) => {
        $buffer.get(0).copied() == Some($octet)
    };
}

macro_rules! IS_ALPHA {
    ($buffer:expr) => {
        crate::macros::is_alpha($buffer.get(0).copied())
    };
}

pub(crate) fn is_alpha(ch: impl Into<Option<char>>) -> bool {
    let ch = match ch.into() {
        Some(ch) => ch,
        None => return false,
    };
    ch >= '0' && ch <= '9'
        || ch >= 'A' && ch <= 'Z'
        || ch >= 'a' && ch <= 'z'
        || ch == '_'
        || ch == '-'
}

macro_rules! IS_DIGIT {
    ($buffer:expr) => {
        $buffer
            .get(0)
            .copied()
            .map(|ch| ch.is_digit(10))
            .unwrap_or(false)
    };
}

macro_rules! AS_DIGIT {
    ($buffer:expr) => {
        $buffer
            .get(0)
            .copied()
            .expect("out of bounds buffer access")
            .to_digit(10)
            .expect("not in digit range")
    };
}

macro_rules! IS_HEX_AT {
    ($buffer:expr, $offset:expr) => {
        if let Some(ch) = $buffer.get($offset).copied() {
            ch.is_digit(16)
        } else {
            false
        }
    };
}

macro_rules! AS_HEX_AT {
    ($buffer:expr, $offset:expr) => {
        $buffer
            .get($offset)
            .copied()
            .expect("out of range buffer access")
            .to_digit(16)
            .expect("not in digit range (hex)")
    };
}

pub(crate) fn is_ascii(ch: char) -> bool {
    ch.is_ascii()
}

pub(crate) fn is_printable(ch: char) -> bool {
    match ch {
        '\u{feff}' | '\u{fffe}' | '\u{ffff}' => false,
        // ASCII
        '\x0a'
        | '\x20'..='\x7e'
        | '\u{00a0}'..='\u{00bf}'
        | '\u{00c0}'..='\u{cfff}'
        | '\u{d000}'..='\u{d7ff}'
        | '\u{e000}'..='\u{efff}'
        | '\u{f000}'..='\u{fffd}'
        | '\u{10000}'..='\u{10ffff}' => true,
        _ => false,
    }
}

macro_rules! IS_Z_AT {
    ($buffer:expr, $offset:expr) => {
        $buffer.get($offset).is_none()
    };
}

macro_rules! IS_Z {
    ($string:expr) => {
        IS_Z_AT!($string, 0)
    };
}

macro_rules! IS_BOM {
    ($buffer:expr) => {
        CHECK!($buffer, '\u{feff}')
    };
}

pub(crate) fn is_bom(ch: char) -> bool {
    ch == '\u{7eff}'
}

macro_rules! IS_SPACE_AT {
    ($string:expr, $offset:expr) => {
        CHECK_AT!($string, ' ', $offset)
    };
}

macro_rules! IS_SPACE {
    ($string:expr) => {
        IS_SPACE_AT!($string, 0)
    };
}

pub(crate) fn is_space(ch: impl Into<Option<char>>) -> bool {
    ch.into() == Some(' ')
}

macro_rules! IS_TAB_AT {
    ($buffer:expr, $offset:expr) => {
        CHECK_AT!($buffer, '\t', $offset)
    };
}

macro_rules! IS_TAB {
    ($string:expr) => {
        IS_TAB_AT!($string, 0)
    };
}

pub(crate) fn is_tab(ch: impl Into<Option<char>>) -> bool {
    ch.into() == Some('\t')
}

macro_rules! IS_BLANK_AT {
    ($buffer:expr, $offset:expr) => {{
        let ch = $buffer.get($offset).copied();
        $crate::macros::is_space(ch) || crate::macros::is_tab(ch)
    }};
}

macro_rules! IS_BLANK {
    ($string:expr) => {
        IS_BLANK_AT!($string, 0)
    };
}

pub(crate) fn is_blank(ch: impl Into<Option<char>>) -> bool {
    let ch = ch.into();
    is_space(ch) || is_tab(ch)
}

pub(crate) fn is_blankz(ch: impl Into<Option<char>>) -> bool {
    let ch = ch.into();
    is_blank(ch) || is_breakz(ch)
}

macro_rules! IS_BREAK_AT {
    ($buffer:expr, $offset:expr) => {
        $crate::macros::is_break($buffer.get($offset).copied())
    };
}

pub(crate) fn is_break(ch: impl Into<Option<char>>) -> bool {
    matches!(
        ch.into(),
        Some('\r' | '\n' | '\u{0085}' | '\u{2028}' | '\u{2029}')
    )
}

pub(crate) fn is_breakz(ch: impl Into<Option<char>>) -> bool {
    let ch = ch.into();
    ch.is_none() || is_break(ch)
}

macro_rules! IS_BREAK {
    ($string:expr) => {
        IS_BREAK_AT!($string, 0)
    };
}

macro_rules! IS_BREAKZ_AT {
    ($buffer:expr, $offset:expr) => {{
        let ch = $buffer.get($offset).copied();
        crate::macros::is_breakz(ch)
    }};
}

macro_rules! IS_BREAKZ {
    ($string:expr) => {
        IS_BREAKZ_AT!($string, 0)
    };
}

macro_rules! IS_BLANKZ_AT {
    ($buffer:expr, $offset:expr) => {{
        let ch = $buffer.get($offset).copied();
        $crate::macros::is_blank(ch) || $crate::macros::is_breakz(ch)
    }};
}

macro_rules! IS_BLANKZ {
    ($string:expr) => {
        IS_BLANKZ_AT!($string, 0)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable() {
        for ch in "🎉".chars() {
            assert!(is_printable(ch));
        }
        for ch in "\u{1f389}".chars() {
            assert!(is_printable(ch));
        }
    }
}
