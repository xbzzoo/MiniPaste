//! 文本子类型识别（纯函数，便于单测）。判定顺序：url > email > color > path。

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// 带协议头或 `www.` 开头的单行串。不认裸域名，避免把 `a.b` 之类误判为链接。
static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(https?|ftp|file)://\S+$|^www\.\S+\.\S+$").expect("url regex")
});

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\w.+\-\u{4e00}-\u{9fa5}]+@[A-Za-z0-9\-]+(\.[A-Za-z0-9\-]+)+$")
        .expect("email regex")
});

static HEX_COLOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$").expect("hex regex")
});

/// 颜色函数。括号内只放行数字、字母、`%` `.` `,` `/` `-` `+` 和空白：
/// 不允许嵌套括号，因此不会出现 `url(...)`，前端可以放心把它塞进 `background`。
static COLOR_FN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(rgba?|hsla?|hwb|lab|lch|oklab|oklch)\(\s*[0-9a-z.%,/+\-\s]+\)$")
        .expect("color fn regex")
});

/// 返回 `url` / `email` / `color` / `path`，都不命中返回 `None`（普通文本）。
///
/// `path` 只认存在的绝对路径：相对路径取决于进程 cwd，判断不可靠。
pub fn text_sub_kind(text: &str) -> Option<&'static str> {
    let value = text.trim();
    if value.is_empty() || value.contains('\n') {
        return None;
    }

    if URL_RE.is_match(value) {
        return Some("url");
    }
    if EMAIL_RE.is_match(value) {
        return Some("email");
    }
    if HEX_COLOR_RE.is_match(value) || COLOR_FN_RE.is_match(value) {
        return Some("color");
    }
    let path = Path::new(value);
    if path.is_absolute() && path.exists() {
        return Some("path");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::text_sub_kind;

    #[test]
    fn detects_url() {
        for s in [
            "https://example.com",
            "http://a.b/c?d=1",
            "www.example.com",
            "  https://x.io  ",
        ] {
            assert_eq!(text_sub_kind(s), Some("url"), "{s:?}");
        }
        assert_eq!(text_sub_kind("example.com"), None);
        assert_eq!(text_sub_kind("hello world"), None);
    }

    #[test]
    fn detects_email() {
        assert_eq!(text_sub_kind("user.name+tag@example.com"), Some("email"));
        assert_eq!(text_sub_kind("张三@example.com.cn"), Some("email"));
        assert_eq!(text_sub_kind("not@an@email"), None);
    }

    #[test]
    fn detects_color() {
        for s in [
            "#fff",
            "#FFFFFF",
            "#ffffff80",
            "rgb(1, 2, 3)",
            "rgba(1,2,3,.5)",
            "hsl(0 100% 50% / 80%)",
            "oklch(0.7 0.15 30)",
        ] {
            assert_eq!(text_sub_kind(s), Some("color"), "{s:?}");
        }
        assert_eq!(text_sub_kind("#xyz"), None);
        assert_eq!(text_sub_kind("rgb(url(x))"), None);
    }

    #[test]
    fn detects_existing_absolute_path_only() {
        let tmp = std::env::temp_dir();
        assert_eq!(text_sub_kind(tmp.to_str().unwrap()), Some("path"));
        assert_eq!(text_sub_kind("/definitely/not/here/xyz"), None);
        assert_eq!(text_sub_kind("src"), None);
    }

    #[test]
    fn multiline_is_plain_text() {
        assert_eq!(text_sub_kind("https://a.com\nhttps://b.com"), None);
    }
}
