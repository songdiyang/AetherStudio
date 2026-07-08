//! REQ-P2-03: Unicode 字符显示宽度精确计算
//!
//! 实现 Unicode East Asian Width 属性的精简版，覆盖 PRD 验收标准：
//! - 重音拉丁字符（à, é）宽度为 1
//! - CJK 字符宽度为 2
//! - Emoji 宽度正确处理（2）
//! - 组合符零宽度
//!
//! 算法基于 Unicode 标准 Annex #11（East Asian Width）的主要范围，
//! 牺牲少量边缘覆盖率换取零外部依赖与更快的编译速度。

/// 返回字符的显示宽度（单位：半角字符宽度）
///
/// - 0：组合标记、控制字符、格式控制符（零宽度）
/// - 1：窄字符（拉丁、希腊、西里尔等）
/// - 2：宽字符（CJK、全角、Emoji）
pub fn char_width(c: char) -> usize {
    let cp = c as u32;

    // ===== 零宽度字符 =====
    if is_zero_width(cp) {
        return 0;
    }

    // ===== 宽字符（East Asian Wide / Fullwidth）=====
    if is_wide(cp) {
        return 2;
    }

    // ===== 其余为窄字符 =====
    1
}

/// 计算字符串的显示宽度（所有字符宽度之和）
pub fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// 判断字符是否为零宽度（组合标记、控制字符、格式控制符）
fn is_zero_width(cp: u32) -> bool {
    // 控制字符
    if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
        return true;
    }

    // 组合标记（Combining Diacritical Marks）
    if (0x0300..=0x036F).contains(&cp) {
        return true;
    }

    // 希伯来语/阿拉伯语组合标记
    if (0x0591..=0x05BD).contains(&cp)
        || cp == 0x05BF
        || (0x05C1..=0x05C2).contains(&cp)
        || (0x05C4..=0x05C5).contains(&cp)
        || cp == 0x05C7
        || (0x0610..=0x061A).contains(&cp)
        || (0x064B..=0x065F).contains(&cp)
        || cp == 0x0670
        || (0x06D6..=0x06DC).contains(&cp)
        || (0x06DF..=0x06E4).contains(&cp)
        || (0x06E7..=0x06E8).contains(&cp)
        || (0x06EA..=0x06ED).contains(&cp)
        || cp == 0x0711
        || (0x0730..=0x074A).contains(&cp)
        || (0x07A6..=0x07B0).contains(&cp)
        || (0x07EB..=0x07F3).contains(&cp)
        || (0x0816..=0x0819).contains(&cp)
        || (0x081B..=0x0823).contains(&cp)
        || (0x0825..=0x0827).contains(&cp)
        || (0x0829..=0x082D).contains(&cp)
        || (0x0859..=0x085B).contains(&cp)
        || (0x08D4..=0x08E1).contains(&cp)
        || (0x08E3..=0x0902).contains(&cp)
        || cp == 0x093A
        || cp == 0x093C
        || (0x0941..=0x0948).contains(&cp)
        || cp == 0x094D
        || (0x0951..=0x0957).contains(&cp)
        || (0x0962..=0x0963).contains(&cp)
        || cp == 0x0981
        || cp == 0x09BC
        || (0x09C1..=0x09C4).contains(&cp)
        || cp == 0x09CD
        || (0x09E2..=0x09E3).contains(&cp)
        || (0x0A01..=0x0A02).contains(&cp)
        || cp == 0x0A3C
        || (0x0A41..=0x0A42).contains(&cp)
        || (0x0A47..=0x0A48).contains(&cp)
        || (0x0A4B..=0x0A4D).contains(&cp)
        || cp == 0x0A51
        || (0x0A70..=0x0A71).contains(&cp)
        || cp == 0x0A75
    {
        return true;
    }

    // 继续覆盖 Devanagari / Bengali / Gurmukhi / Gujarati / Oriya / Tamil / Telugu / Kannada / Malayalam / Sinhala / Thai / Lao / Tibetan / Myanmar 组合标记
    if (0x0A81..=0x0A82).contains(&cp)
        || cp == 0x0ABC
        || (0x0AC1..=0x0AC5).contains(&cp)
        || (0x0AC7..=0x0AC8).contains(&cp)
        || cp == 0x0ACD
        || (0x0AE2..=0x0AE3).contains(&cp)
        || cp == 0x0B01
        || cp == 0x0B3C
        || cp == 0x0B3F
        || (0x0B41..=0x0B44).contains(&cp)
        || cp == 0x0B4D
        || cp == 0x0B56
        || (0x0B62..=0x0B63).contains(&cp)
        || cp == 0x0B82
        || cp == 0x0BC0
        || cp == 0x0BCD
        || cp == 0x0C00
        || (0x0C3E..=0x0C40).contains(&cp)
        || (0x0C46..=0x0C48).contains(&cp)
        || (0x0C4A..=0x0C4D).contains(&cp)
        || (0x0C55..=0x0C56).contains(&cp)
        || (0x0C62..=0x0C63).contains(&cp)
        || cp == 0x0C81
        || cp == 0x0CBC
        || cp == 0x0CBF
        || cp == 0x0CC6
        || (0x0CCC..=0x0CCD).contains(&cp)
        || (0x0CE2..=0x0CE3).contains(&cp)
        || (0x0D00..=0x0D01).contains(&cp)
        || (0x0D3B..=0x0D3C).contains(&cp)
        || (0x0D41..=0x0D44).contains(&cp)
        || cp == 0x0D4D
        || (0x0D62..=0x0D63).contains(&cp)
        || cp == 0x0DCA
        || (0x0DD2..=0x0DD4).contains(&cp)
        || cp == 0x0DD6
        || cp == 0x0E31
        || (0x0E34..=0x0E3A).contains(&cp)
        || (0x0E47..=0x0E4E).contains(&cp)
        || cp == 0x0EB1
        || (0x0EB4..=0x0EB9).contains(&cp)
        || (0x0EBB..=0x0EBC).contains(&cp)
        || (0x0EC8..=0x0ECD).contains(&cp)
        || (0x0F18..=0x0F19).contains(&cp)
        || cp == 0x0F35
        || cp == 0x0F37
        || cp == 0x0F39
        || (0x0F71..=0x0F7E).contains(&cp)
        || (0x0F80..=0x0F84).contains(&cp)
        || (0x0F86..=0x0F87).contains(&cp)
        || (0x0F8D..=0x0F97).contains(&cp)
        || (0x0F99..=0x0FBC).contains(&cp)
        || cp == 0x0FC6
    {
        return true;
    }

    // 通用组合标记范围（一次性覆盖大多数补充分块）
    if (0x1AB0..=0x1AFF).contains(&cp) {
        return true;
    }

    // 变体选择符（VS1-VS16, VS17-VS256）
    if (0xFE00..=0xFE0F).contains(&cp) || (0xE0100..=0xE01EF).contains(&cp) {
        return true;
    }

    // 字形变体选择符（Mongolian Variation Selectors）
    if (0x180B..=0x180D).contains(&cp) || cp == 0x180F {
        return true;
    }

    // 零宽空格家族
    if (0x200B..=0x200F).contains(&cp) {
        return true;
    }

    // 双向格式控制
    if (0x202A..=0x202E).contains(&cp) {
        return true;
    }

    // 通用格式控制字符
    if (0x2060..=0x2064).contains(&cp) || (0x2066..=0x206F).contains(&cp) {
        return true;
    }

    // BOM
    if cp == 0xFEFF {
        return true;
    }

    // 组合附加符号（Spacing Modifier Letters 的部分组合字符）
    if (0x1DC0..=0x1DFF).contains(&cp) {
        return true;
    }

    // 组合符号用附加符号
    if (0x20D0..=0x20FF).contains(&cp) {
        return true;
    }

    // CJK 组合标记（声调符号）
    if (0x302A..=0x302D).contains(&cp) || (0x3099..=0x309A).contains(&cp) {
        return true;
    }

    // 音乐符号组合
    if (0x1D167..=0x1D169).contains(&cp)
        || (0x1D17B..=0x1D182).contains(&cp)
        || (0x1D185..=0x1D18B).contains(&cp)
        || (0x1D1AA..=0x1D1AD).contains(&cp)
    {
        return true;
    }

    // 数学符号组合
    if (0xFE20..=0xFE2F).contains(&cp) {
        return true;
    }

    false
}

/// 判断字符是否为宽字符（East Asian Wide / Fullwidth / Emoji）
fn is_wide(cp: u32) -> bool {
    // ===== East Asian Wide / Fullwidth =====

    // CJK 标点符号
    if (0x3000..=0x303E).contains(&cp) {
        return true;
    }

    // Hiragana
    if (0x3041..=0x3096).contains(&cp) {
        return true;
    }

    // Katakana
    if (0x30A1..=0x30FA).contains(&cp) || (0x30FC..=0x30FF).contains(&cp) {
        return true;
    }

    // Bopomofo
    if (0x3105..=0x312F).contains(&cp) {
        return true;
    }

    // Hangul Compatibility Jamo
    if (0x3131..=0x318E).contains(&cp) {
        return true;
    }

    // CJK Unified Ideographs Extension A
    if (0x3400..=0x4DBF).contains(&cp) {
        return true;
    }

    // CJK Unified Ideographs
    if (0x4E00..=0x9FFF).contains(&cp) {
        return true;
    }

    // Yi Syllables / Radicals
    if (0xA000..=0xA4CF).contains(&cp) {
        return true;
    }

    // Hangul Syllables
    if (0xAC00..=0xD7A3).contains(&cp) {
        return true;
    }

    // CJK Compatibility Ideographs
    if (0xF900..=0xFAFF).contains(&cp) {
        return true;
    }

    // CJK Compatibility Forms
    if (0xFE30..=0xFE4F).contains(&cp) {
        return true;
    }

    // Fullwidth Forms
    if (0xFF01..=0xFF60).contains(&cp) || (0xFFE0..=0xFFE6).contains(&cp) {
        return true;
    }

    // Halfwidth and Fullwidth Forms 中的 fullwidth 部分已在上面覆盖

    // ===== CJK Extension B-F, H, I, J, K, etc. =====
    if (0x20000..=0x2FFFD).contains(&cp) || (0x30000..=0x3FFFD).contains(&cp) {
        return true;
    }

    // ===== Emoji 宽度处理 =====
    // 注：Unicode 标准中 Emoji 的 East Asian Width 属性为 W（Wide）
    if is_emoji(cp) {
        return true;
    }

    false
}

/// 判断字符是否为 Emoji（基于 Unicode Emoji 数据的主要范围）
fn is_emoji(cp: u32) -> bool {
    // Emoticons
    if (0x1F600..=0x1F64F).contains(&cp) {
        return true;
    }
    // Miscellaneous Symbols and Pictographs
    if (0x1F300..=0x1F5FF).contains(&cp) {
        return true;
    }
    // Transport and Map Symbols
    if (0x1F680..=0x1F6FF).contains(&cp) {
        return true;
    }
    // Supplemental Symbols and Pictographs
    if (0x1F900..=0x1F9FF).contains(&cp) {
        return true;
    }
    // Symbols and Pictographs Extended-A
    if (0x1FA70..=0x1FAFF).contains(&cp) {
        return true;
    }
    // Dingbats（部分被 Emoji 化）
    if (0x2700..=0x27BF).contains(&cp) {
        return true;
    }
    // Miscellaneous Symbols（部分被 Emoji 化）
    if (0x2600..=0x26FF).contains(&cp) {
        return true;
    }
    // Regional Indicator Symbols（用于国旗）
    if (0x1F1E6..=0x1F1FF).contains(&cp) {
        return true;
    }
    // Emoji Modifier
    if (0x1F9B0..=0x1F9B9).contains(&cp) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_narrow() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('A'), 1);
        assert_eq!(char_width('0'), 1);
        assert_eq!(char_width(' '), 1);
        assert_eq!(char_width('!'), 1);
    }

    #[test]
    fn test_accented_latin_narrow() {
        // REQ-P2-03: 重音拉丁字符宽度为 1
        assert_eq!(char_width('à'), 1);
        assert_eq!(char_width('é'), 1);
        assert_eq!(char_width('ü'), 1);
        assert_eq!(char_width('ñ'), 1);
        assert_eq!(char_width('ç'), 1);
    }

    #[test]
    fn test_cjk_wide() {
        // REQ-P2-03: CJK 字符宽度为 2
        assert_eq!(char_width('中'), 2);
        assert_eq!(char_width('文'), 2);
        assert_eq!(char_width('字'), 2);
        assert_eq!(char_width('日'), 2);
        assert_eq!(char_width('本'), 2);
        assert_eq!(char_width('语'), 2);
        // 平假名
        assert_eq!(char_width('あ'), 2);
        // 片假名
        assert_eq!(char_width('ア'), 2);
        // 谚文
        assert_eq!(char_width('가'), 2);
    }

    #[test]
    fn test_fullwidth_latin() {
        assert_eq!(char_width('Ａ'), 2);
        assert_eq!(char_width('９'), 2);
        assert_eq!(char_width('！'), 2);
    }

    #[test]
    fn test_combining_zero_width() {
        // REQ-P2-03: 组合符零宽度
        assert_eq!(char_width('\u{0300}'), 0); // 组合重音
        assert_eq!(char_width('\u{0301}'), 0); // 组合尖音符
        assert_eq!(char_width('\u{0308}'), 0); // 组合分音符
        assert_eq!(char_width('\u{200B}'), 0); // 零宽空格
        assert_eq!(char_width('\u{FEFF}'), 0); // BOM
    }

    #[test]
    fn test_emoji_wide() {
        // REQ-P2-03: Emoji 宽度正确处理
        assert_eq!(char_width('😀'), 2);
        assert_eq!(char_width('🎉'), 2);
        assert_eq!(char_width('❤'), 2);
        assert_eq!(char_width('☀'), 2);
    }

    #[test]
    fn test_control_zero_width() {
        assert_eq!(char_width('\u{0}'), 0);
        assert_eq!(char_width('\u{7}'), 0);
        assert_eq!(char_width('\u{1B}'), 0);
        assert_eq!(char_width('\u{7F}'), 0);
    }

    #[test]
    fn test_str_width() {
        assert_eq!(str_width("hello"), 5);
        assert_eq!(str_width("你好"), 4);
        assert_eq!(str_width("a中b"), 4);
        assert_eq!(str_width("a\u{0301}b"), 2); // a + 组合尖音 + b
    }

    #[test]
    fn test_cjk_extension() {
        assert_eq!(char_width('𠀀'), 2); // CJK Ext B
        assert_eq!(char_width('𪜶'), 2); // CJK Ext C
    }
}
