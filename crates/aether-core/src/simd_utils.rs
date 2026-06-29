/// SIMD加速的文本处理工具
///
/// 使用 128 位（16字节）和 64 位（8字节）批量处理，模拟 SIMD 效果
/// 在稳定版 Rust 中无需外部依赖即可实现

/// 快速计算字节数组中的换行符数量
///
/// 使用 16 字节批量处理（u128），比 8 字节版本快 ~2 倍
/// CORE-C01: 修复 SWAR 跨字节借位导致的误计 — 使用 has_zero_byte 做快速跳过，
/// 在有换行符的块内逐字节精确计数，避免 borrow artifact
pub fn count_newlines_simd(data: &[u8]) -> u32 {
    let mut count = 0u32;
    let len = data.len();
    let mut i = 0;

    // 16 字节批量处理（u128）
    while i + 16 <= len {
        let chunk = u128::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
            data[i + 8],
            data[i + 9],
            data[i + 10],
            data[i + 11],
            data[i + 12],
            data[i + 13],
            data[i + 14],
            data[i + 15],
        ]);

        let xor_result = chunk ^ 0x0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0Au128;
        // has_zero_byte 仅用于快速检测（可能有 borrow artifact，不能直接 count_ones）
        if has_zero_byte_u128(xor_result) != 0 {
            // 块内有换行符 — 逐字节精确计数，避免借位传播误计
            for j in 0..16 {
                if data[i + j] == b'\n' {
                    count += 1;
                }
            }
        }
        i += 16;
    }

    // 8 字节处理剩余部分
    while i + 8 <= len {
        let chunk = u64::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
        ]);
        let xor_result = chunk ^ 0x0A0A0A0A0A0A0A0Au64;
        if has_zero_byte(xor_result) != 0 {
            for j in 0..8 {
                if data[i + j] == b'\n' {
                    count += 1;
                }
            }
        }
        i += 8;
    }

    // 处理剩余字节
    while i < len {
        if data[i] == b'\n' {
            count += 1;
        }
        i += 1;
    }

    count
}

/// 快速查找字节在数组中的位置
///
/// 使用 16 字节批量比较加速
pub fn find_byte_simd(data: &[u8], target: u8) -> Option<usize> {
    let len = data.len();
    let mut i = 0;

    // 16 字节批量处理
    let pattern_128 = u128::from_le_bytes([target; 16]);

    while i + 16 <= len {
        let chunk = u128::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
            data[i + 8],
            data[i + 9],
            data[i + 10],
            data[i + 11],
            data[i + 12],
            data[i + 13],
            data[i + 14],
            data[i + 15],
        ]);

        let xor_result = chunk ^ pattern_128;
        let is_zero = has_zero_byte_u128(xor_result);

        if is_zero != 0 {
            let tz = is_zero.trailing_zeros();
            return Some(i + (tz / 8) as usize);
        }

        i += 16;
    }

    // 8 字节批量处理
    let pattern_64 = u64::from_le_bytes([target; 8]);

    while i + 8 <= len {
        let chunk = u64::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
        ]);

        let xor_result = chunk ^ pattern_64;
        let is_zero = has_zero_byte(xor_result);

        if is_zero != 0 {
            let tz = is_zero.trailing_zeros();
            return Some(i + (tz / 8) as usize);
        }

        i += 8;
    }

    // 处理剩余字节
    while i < len {
        if data[i] == target {
            return Some(i);
        }
        i += 1;
    }

    None
}

/// 快速跳过空白字符
///
/// 16 字节批量检查空格、制表符、回车
pub fn skip_whitespace_simd(data: &[u8], start: usize) -> usize {
    let len = data.len();
    let mut i = start;

    // 16 字节批量检测
    while i + 16 <= len {
        let chunk = u128::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
            data[i + 8],
            data[i + 9],
            data[i + 10],
            data[i + 11],
            data[i + 12],
            data[i + 13],
            data[i + 14],
            data[i + 15],
        ]);

        let is_space = chunk ^ 0x20202020202020202020202020202020u128;
        let is_tab = chunk ^ 0x09090909090909090909090909090909u128;
        let is_cr = chunk ^ 0x0D0D0D0D0D0D0D0D0D0D0D0D0D0D0D0Du128;

        let zero_space = has_zero_byte_u128(is_space);
        let zero_tab = has_zero_byte_u128(is_tab);
        let zero_cr = has_zero_byte_u128(is_cr);

        let is_whitespace = zero_space | zero_tab | zero_cr;

        if is_whitespace != 0x80808080808080808080808080808080u128 {
            // 不是所有字节都是空白，逐个处理
            break;
        }

        i += 16;
    }

    // 8 字节批量检测
    while i + 8 <= len {
        let chunk = u64::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
        ]);

        let is_space = chunk ^ 0x2020202020202020u64;
        let is_tab = chunk ^ 0x0909090909090909u64;
        let is_cr = chunk ^ 0x0D0D0D0D0D0D0D0Du64;

        let zero_space = has_zero_byte(is_space);
        let zero_tab = has_zero_byte(is_tab);
        let zero_cr = has_zero_byte(is_cr);

        let is_whitespace = zero_space | zero_tab | zero_cr;

        if is_whitespace != 0x8080808080808080u64 {
            break;
        }

        i += 8;
    }

    // 逐个处理剩余字节
    while i < len {
        match data[i] {
            b' ' | b'\t' | b'\r' => i += 1,
            _ => break,
        }
    }

    i
}

/// 检测 128 位整数中是否有 0 字节
#[inline(always)]
fn has_zero_byte_u128(x: u128) -> u128 {
    let sub = x.wrapping_sub(0x01010101010101010101010101010101u128);
    let not_x = !x;
    sub & not_x & 0x80808080808080808080808080808080u128
}

/// 检测64位整数中是否有0字节
#[inline(always)]
fn has_zero_byte(x: u64) -> u64 {
    let sub = x.wrapping_sub(0x0101010101010101u64);
    let not_x = !x;
    sub & not_x & 0x8080808080808080u64
}

/// 快速字符串前缀匹配（用于关键字检测）
///
/// 使用 8 字节批量比较（升级为 64 位）
pub fn starts_with_simd(data: &[u8], prefix: &[u8]) -> bool {
    if data.len() < prefix.len() {
        return false;
    }

    let prefix_len = prefix.len();
    let mut i = 0;

    // 8 字节批量比较
    while i + 8 <= prefix_len {
        let data_chunk = u64::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
        ]);
        let prefix_chunk = u64::from_le_bytes([
            prefix[i],
            prefix[i + 1],
            prefix[i + 2],
            prefix[i + 3],
            prefix[i + 4],
            prefix[i + 5],
            prefix[i + 6],
            prefix[i + 7],
        ]);
        if data_chunk != prefix_chunk {
            return false;
        }
        i += 8;
    }

    // 4 字节批量比较
    while i + 4 <= prefix_len {
        let data_chunk = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        let prefix_chunk =
            u32::from_le_bytes([prefix[i], prefix[i + 1], prefix[i + 2], prefix[i + 3]]);
        if data_chunk != prefix_chunk {
            return false;
        }
        i += 4;
    }

    // 比较剩余字节
    while i < prefix_len {
        if data[i] != prefix[i] {
            return false;
        }
        i += 1;
    }

    true
}

/// 快速计算字符串长度（到下一个换行符）
///
/// 使用SIMD批量查找换行符
pub fn line_length_simd(data: &[u8], start: usize) -> usize {
    match find_byte_simd(&data[start..], b'\n') {
        Some(pos) => pos,
        None => data.len() - start,
    }
}

/// 批量检测字符类型（用于lexer）
///
/// 返回每个字节的字符类型分类
/// 类型：0=其他, 1=字母, 2=数字, 3=空白
#[allow(dead_code)]
pub fn classify_chars_simd(data: &[u8], start: usize, out: &mut [u8]) {
    let len = data.len().saturating_sub(start).min(out.len());
    let mut i = 0;

    // 16 字节批量分类
    while i + 16 <= len {
        for j in 0..16 {
            out[i + j] = classify_byte(data[start + i + j]);
        }
        i += 16;
    }

    // 8 字节批量分类
    while i + 8 <= len {
        for j in 0..8 {
            out[i + j] = classify_byte(data[start + i + j]);
        }
        i += 8;
    }

    // 处理剩余字节
    while i < len {
        out[i] = classify_byte(data[start + i]);
        i += 1;
    }
}

#[inline(always)]
fn classify_byte(byte: u8) -> u8 {
    match byte {
        b'a'..=b'z' | b'A'..=b'Z' | b'_' => 1, // 字母/标识符
        b'0'..=b'9' => 2,                      // 数字
        b' ' | b'\t' | b'\r' | b'\n' => 3,     // 空白
        _ => 0,                                // 其他
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_newlines_simd() {
        let data = b"line1\nline2\nline3\n";
        assert_eq!(count_newlines_simd(data), 3);

        let data2 = b"no newlines here";
        assert_eq!(count_newlines_simd(data2), 0);

        let data3 = b"\n\n\n";
        assert_eq!(count_newlines_simd(data3), 3);
    }

    #[test]
    fn test_find_byte_simd() {
        let data = b"hello world\nfoo";
        assert_eq!(find_byte_simd(data, b'\n'), Some(11));
        assert_eq!(find_byte_simd(data, b'x'), None);
        assert_eq!(find_byte_simd(data, b'h'), Some(0));
    }

    #[test]
    fn test_skip_whitespace_simd() {
        let data = b"   \t\t  hello";
        assert_eq!(skip_whitespace_simd(data, 0), 7);

        let data2 = b"hello";
        assert_eq!(skip_whitespace_simd(data2, 0), 0);
    }

    #[test]
    fn test_starts_with_simd() {
        assert!(starts_with_simd(b"hello world", b"hello"));
        assert!(!starts_with_simd(b"hello world", b"world"));
        assert!(starts_with_simd(b"fn main()", b"fn"));
    }

    #[test]
    fn test_large_file_newlines() {
        // 测试大文件场景
        let mut data = Vec::with_capacity(10000);
        for i in 0..1000 {
            data.extend_from_slice(format!("line {}\n", i).as_bytes());
        }

        let simd_count = count_newlines_simd(&data);
        let scalar_count = data.iter().filter(|&&b| b == b'\n').count() as u32;
        assert_eq!(simd_count, scalar_count);
    }

    #[test]
    fn test_16byte_boundary() {
        // 测试 16 字节边界情况
        let data = b"0123456789abcdef\nmore";
        assert_eq!(find_byte_simd(data, b'\n'), Some(16));

        let data2 = b"0123456789abcde\nmore";
        assert_eq!(find_byte_simd(data2, b'\n'), Some(15));

        let data3 = b"0123456789abcdefg\nmore";
        assert_eq!(find_byte_simd(data3, b'\n'), Some(17));
    }

    #[test]
    fn test_count_newlines_large() {
        // 测试大数据（> 32 字节，确保 16 字节路径生效）
        let mut data = vec![b'a'; 128];
        data[15] = b'\n';
        data[31] = b'\n';
        data[63] = b'\n';
        data[127] = b'\n';
        assert_eq!(count_newlines_simd(&data), 4);
    }
}
