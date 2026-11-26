//! MidgardCP (web control panel) for the Patch Maker: news from the panel's public API go into the patch list, so they
//! are written once in the panel. The panel never signs anything; the signing key stays with the Patch Maker.
//!
//! `news_from` accepts MidgardCP's `GET /api/v1/news?realm=<id>&limit=<n>` (`{"items":[{"title","html","published_at"}]}`)
//! and, for other panels, a plain list in the patcher's own format (`[{"date","title","body"}]`).

use crate::patchlist::NewsItem;
use crate::{msg, Result};

/// Turns the panel's sanitised HTML into the patcher's tiny markup: blank line = paragraph, `- ` = list item, `**bold**`
/// (also headings); table rows become `cell | cell` lines; links keep their text and get the address in brackets
/// (relative ones made absolute against `base`, the panel's address).
pub fn html_to_markup(html: &str, base: &str) -> String {
    let origin = base.split_once("://").map(|(s, rest)| format!("{s}://{}", rest.split('/').next().unwrap_or(""))).unwrap_or_default();
    let mut out = String::new();
    let mut rest = html;
    let mut href: Vec<String> = Vec::new();
    let mut cell = 0usize;
    let mut pre = 0usize;
    let text = |t: &str, pre: usize| -> String {
        let t = entities(t);
        if pre > 0 {
            return t;
        }
        // whitespace as a browser shows it: any run is one space
        let mut o = String::with_capacity(t.len());
        let mut sp = false;
        for c in t.chars() {
            if c.is_whitespace() {
                if !sp {
                    o.push(' ');
                }
                sp = true;
            } else {
                o.push(c);
                sp = false;
            }
        }
        o
    };
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push_str(&text(rest, pre));
            break;
        };
        out.push_str(&text(&rest[..lt], pre));
        let Some(gt) = rest[lt..].find('>') else {
            out.push_str(&text(&rest[lt..], pre));
            break;
        };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match (name.as_str(), closing) {
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => out.push_str("\n\n**"),
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => out.push_str("**\n\n"),
            ("pre", false) => {
                pre += 1;
                out.push_str("\n\n");
            }
            ("pre", true) => {
                pre = pre.saturating_sub(1);
                out.push_str("\n\n");
            }
            ("p" | "div" | "ul" | "ol" | "blockquote" | "table", _) => out.push_str("\n\n"),
            ("br", _) => out.push('\n'),
            ("tr", false) => {
                out.push('\n');
                cell = 0;
            }
            ("td" | "th", false) => {
                if cell > 0 {
                    out.push_str(" | ");
                }
                cell += 1;
            }
            ("li", false) => out.push_str("\n- "),
            ("strong" | "b", _) => out.push_str("**"),
            ("a", false) => {
                let h = tag.split("href=").nth(1).map(|v| v.trim_start_matches(['"', '\'']).split(['"', '\'', ' ']).next().unwrap_or("").to_string()).unwrap_or_default();
                let h = entities(&h);
                href.push(if h.starts_with('/') && !h.starts_with("//") && !origin.is_empty() { format!("{origin}{h}") } else { h });
            }
            ("a", true) => {
                if let Some(h) = href.pop().filter(|h| h.starts_with("https://") || h.starts_with("http://")) {
                    out.push_str(&format!(" ({h})"));
                }
            }
            _ => {}
        }
    }
    // tidy: no spaces at line ends, at most one blank line, no blank line between list items
    let mut lines: Vec<String> = Vec::new();
    for l in out.lines().map(|l| l.trim().to_string()) {
        let blank = l.is_empty() || l == "****";
        if blank && lines.last().is_none_or(|p: &String| p.is_empty()) {
            continue;
        }
        // a heading line is "**text**": drop the spaces the source had inside the tags
        let l = match l.strip_prefix("**").and_then(|x| x.strip_suffix("**")) {
            Some(inner) if !inner.contains("**") => format!("**{}**", inner.trim()),
            _ => l,
        };
        lines.push(if blank { String::new() } else { l });
    }
    let mut kept: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let between_items = l.is_empty() && kept.last().is_some_and(|p| p.starts_with("- ")) && lines.get(i + 1).is_some_and(|n| n.starts_with("- "));
        if !between_items {
            kept.push(l.clone());
        }
    }
    kept.join("\n").trim().to_string()
}

fn entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';').filter(|j| *j <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let e = &rest[1..j];
        let c = match e {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" | "#x27" => Some('\''),
            "nbsp" => Some(' '),
            _ if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            _ if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// News items from a panel answer (MidgardCP or the patcher's own format), newest first as the panel sends them. `base`
/// is the address the answer came from (for relative links).
pub fn parse_news(bytes: &[u8], base: &str) -> Result<Vec<NewsItem>> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    let items = v.get("items").and_then(|i| i.as_array()).or_else(|| v.as_array()).ok_or_else(|| msg("the news answer has no item list"))?;
    let mut out = Vec::new();
    for it in items {
        let title = it["title"].as_str().unwrap_or("").trim().to_string();
        if title.is_empty() {
            continue;
        }
        let date = it["date"].as_str().or_else(|| it["published_at"].as_str()).unwrap_or("").chars().take(10).collect();
        let body = match (it["body"].as_str(), it["html"].as_str()) {
            (Some(b), _) => b.to_string(),
            (None, Some(h)) => html_to_markup(h, base),
            _ => String::new(),
        };
        let lang = it["lang"].as_str().or_else(|| it["locale"].as_str()).unwrap_or("").trim().to_ascii_lowercase();
        out.push(NewsItem { date, title, body, lang });
    }
    Ok(out)
}

/// Fetches the news from a panel URL (`https://cp.example/api/v1/news?realm=hr&limit=10`).
pub fn fetch_news(url: &str) -> Result<Vec<NewsItem>> {
    parse_news(&crate::fetch::get_bytes(url)?, url)
}

/// The state of one realm in a MidgardCP status answer (`GET /api/v1/status`).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct RealmState {
    pub name: String,
    /// `online`, `degraded`, `offline` or `unknown`
    pub state: String,
    pub players: Option<u64>,
}

pub fn parse_status(bytes: &[u8], realm: &str) -> Result<RealmState> {
    let v: serde_json::Value = serde_json::from_slice(bytes)?;
    let list = v.as_array().cloned().unwrap_or_else(|| vec![v]);
    let r = list
        .iter()
        .find(|r| realm.is_empty() || r["id"].as_str() == Some(realm))
        .ok_or_else(|| msg(format!("realm {realm} is not in the status answer")))?;
    Ok(RealmState {
        name: r["name"].as_str().or_else(|| r["id"].as_str()).unwrap_or("").to_string(),
        state: r["state"].as_str().unwrap_or("unknown").to_string(),
        players: r["players"].as_u64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_patcher_markup() {
        let h = "<p>WoE starts <strong>Sunday</strong> &amp; Wednesday.</p><ul><li>Castles: Prontera</li><li>Time: 19&#58;00</li></ul><p>See <a href=\"https://burningro.example/woe\">the guide</a>.</p>";
        assert_eq!(
            html_to_markup(h, ""),
            "WoE starts **Sunday** & Wednesday.\n\n- Castles: Prontera\n- Time: 19:00\n\nSee the guide (https://burningro.example/woe)."
        );
    }

    #[test]
    fn midgardcp_markdown_output() {
        // as MidgardCP renders a news article (heading, table, list, relative link, source line breaks)
        let h = "<h2>Neue WoE-Zeiten</h2>\n<p>Ab dem <strong>1. Dezember</strong> l\u{e4}uft die WoE.</p>\n<table><thead><tr><th>Tag</th><th>Zeit</th></tr></thead><tbody>\n<tr><td>Sa</td><td>20:00</td></tr>\n<tr><td>So</td><td>19:00</td></tr>\n</tbody></table>\n<ul>\n<li>Belohnungen \u{fc}ber die <a href=\"/me\" rel=\"noopener\">Inbox</a></li>\n<li>Fragen? Discord.</li>\n</ul>\n";
        assert_eq!(
            html_to_markup(h, "https://cp.burningro.example/api/v1/news?realm=hr"),
            "**Neue WoE-Zeiten**\n\nAb dem **1. Dezember** l\u{e4}uft die WoE.\n\nTag | Zeit\nSa | 20:00\nSo | 19:00\n\n- Belohnungen \u{fc}ber die Inbox (https://cp.burningro.example/me)\n- Fragen? Discord."
        );
    }

    #[test]
    fn midgardcp_news_and_status() {
        let n = parse_news(br#"{"total":2,"items":[{"id":2,"title":"Patch day","html":"<p>New maps</p>","realm":null,"pinned":false,"published_at":"2026-11-26T10:00:00Z"},{"id":1,"title":"","html":"x"}]}"#, "").unwrap();
        assert_eq!(n, vec![NewsItem { date: "2026-11-26".into(), title: "Patch day".into(), body: "New maps".into(), lang: String::new() }]);
        let own = parse_news(br#"[{"date":"2026-11-01","title":"Hi","body":"- a"}]"#, "").unwrap();
        assert_eq!(own[0].body, "- a");
        let langs = parse_news(br#"[{"title":"Hallo","body":"x","lang":"DE"},{"title":"Hi","body":"x","locale":"en"}]"#, "").unwrap();
        assert_eq!((langs[0].lang.as_str(), langs[1].lang.as_str()), ("de", "en"), "the panel's language travels with the item");
        let s = br#"[{"id":"hr","name":"BurningRO HR","state":"online","players":42},{"id":"mlr","name":"M-LR","state":"degraded","players":null}]"#;
        assert_eq!(parse_status(s, "mlr").unwrap(), RealmState { name: "M-LR".into(), state: "degraded".into(), players: None });
        assert_eq!(parse_status(s, "").unwrap().players, Some(42));
        assert!(parse_status(s, "lr").is_err());
    }
}
