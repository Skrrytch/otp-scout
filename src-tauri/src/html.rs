use regex::Regex;

pub fn strip_html(input: &str) -> String {
    let mut out = input.to_string();

    out = out.replace("<br>", "\n");
    out = out.replace("<br/>", "\n");
    out = out.replace("<br />", "\n");
    out = out.replace("<BR>", "\n");
    out = out.replace("<BR/>", "\n");

    let block_tags = Regex::new(r"</?(p|div|tr|li|h[1-6]|table|thead|tbody|td|th|hr|pre|blockquote|section|article|header|footer|nav|main|aside|ul|ol|dl)[^>]*/?>").unwrap();
    out = block_tags.replace_all(&out, "\n").to_string();

    let any_tag = Regex::new(r"<[^>]*>").unwrap();
    out = any_tag.replace_all(&out, "").to_string();

    out = out.replace("&amp;", "&");
    out = out.replace("&lt;", "<");
    out = out.replace("&gt;", ">");
    out = out.replace("&quot;", "\"");
    out = out.replace("&#39;", "'");
    out = out.replace("&apos;", "'");
    out = out.replace("&nbsp;", " ");

    let numeric_entity = Regex::new(r"&#(\d+);").unwrap();
    let mut result = out.clone();
    let mut offset: i64 = 0;
    for caps in numeric_entity.captures_iter(&out) {
        let m = caps.get(0).unwrap();
        let num: u32 = caps[1].parse().unwrap_or(b'?' as u32);
        let ch = char::from_u32(num).unwrap_or('?');
        let start = (m.start() as i64 + offset) as usize;
        let end = (m.end() as i64 + offset) as usize;
        let replacement = ch.to_string();
        let old_len = result.len();
        result.replace_range(start..end, &replacement);
        offset += replacement.len() as i64 - (end - start) as i64;
        let new_len = result.len();
        let actual_diff = new_len as i64 - old_len as i64;
        offset = offset - (replacement.len() as i64 - (end - start) as i64) + actual_diff;
    }

    let empty_line = Regex::new(r"\n{3,}").unwrap();
    result = empty_line.replace_all(&result, "\n\n").to_string();

    result = result.trim().to_string();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_basic_tags() {
        let html = "<p>Hello <b>World</b></p>";
        assert_eq!(strip_html(html), "Hello World");
    }

    #[test]
    fn test_strip_br() {
        let html = "Line1<br>Line2<br/>Line3<br />Line4";
        assert_eq!(strip_html(html), "Line1\nLine2\nLine3\nLine4");
    }

    #[test]
    fn test_decode_entities() {
        let html = "&amp; &lt; &gt; &quot;";
        assert_eq!(strip_html(html), "& < > \"");
    }

    #[test]
    fn test_nbsp() {
        let html = "a&nbsp;b";
        assert_eq!(strip_html(html), "a b");
    }

    #[test]
    fn test_numeric_entity() {
        let html = "&#65; &#66; &#67;";
        assert_eq!(strip_html(html), "A B C");
    }

    #[test]
    fn test_block_tags() {
        let html = "<div>A</div><p>B</p><span>C</span>";
        assert_eq!(strip_html(html), "A\n\nB\nC");
    }

    #[test]
    fn test_trim_empty_lines() {
        let html = "<p>A</p><br><br><p>B</p>";
        let result = strip_html(html);
        assert!(!result.contains("\n\n\n"));
    }
}