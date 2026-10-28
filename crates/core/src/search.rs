use crate::{Cache, Entry, Settings};
use pinyin::ToPinyin;

#[derive(Clone, Debug)]
pub struct SearchEntry {
    pub key: String,
    pub plugin_id: String,
    pub entry: Entry,
    title: String,
    pinyin: String,
    initials: String,
    keywords: String,
}

pub fn build_index(cache: &Cache, settings: &Settings) -> Vec<SearchEntry> {
    let mut output = Vec::new();
    for (plugin, entries) in &cache.plugins {
        if settings.disabled_plugins.contains(plugin) {
            continue;
        }
        for entry in entries {
            let mut pinyin = String::new();
            let mut initials = String::new();
            for c in entry.title.chars() {
                if let Some(py) = c.to_pinyin() {
                    pinyin.push_str(py.plain());
                    if let Some(first) = py.plain().chars().next() {
                        initials.push(first);
                    }
                } else {
                    for lower in c.to_lowercase().filter(|x| !x.is_whitespace()) {
                        pinyin.push(lower);
                        initials.push(lower);
                    }
                }
            }
            output.push(SearchEntry {
                key: format!("{}:{}", plugin, entry.id),
                plugin_id: plugin.clone(),
                entry: entry.clone(),
                title: entry.title.to_lowercase(),
                pinyin,
                initials,
                keywords: entry.keywords.join(" ").to_lowercase(),
            });
        }
    }
    output
}

fn subsequence(needle: &str, haystack: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|c| chars.by_ref().any(|x| x == c))
}

pub fn search(index: &[SearchEntry], query: &str, settings: &Settings, limit: usize) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let terms: Vec<&str> = query.split_whitespace().collect();
    let mut scored = Vec::new();
    for (i, item) in index.iter().enumerate() {
        let mut score = 0i64;
        let mut matches = true;
        for term in &terms {
            let value = if item.title == *term {
                10000
            } else if item.title.starts_with(term) {
                8000
            } else if item.title.contains(term) {
                6000
            } else if item.pinyin.starts_with(term) || item.initials.starts_with(term) {
                5000
            } else if item.pinyin.contains(term)
                || item.initials.contains(term)
                || item.keywords.contains(term)
            {
                3000
            } else if term.len() >= 2 && term.is_ascii() && subsequence(term, &item.title) {
                1000
            } else {
                matches = false;
                break;
            };
            score += value;
        }
        if !matches {
            continue;
        }
        let usage = settings.usage.get(&item.key).cloned().unwrap_or_default();
        scored.push((i, score, usage.count, usage.last_used));
    }
    scored.sort_by(|(a, sa, ca, ta), (b, sb, cb, tb)| {
        cb.cmp(ca)
            .then_with(|| sb.cmp(sa))
            .then_with(|| tb.cmp(ta))
            .then_with(|| index[*a].title.cmp(&index[*b].title))
            .then(a.cmp(b))
    });
    scored.into_iter().take(limit).map(|(i, ..)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Target, Usage, now};
    #[test]
    fn frequency_orders_only_matching_entries_without_count_cap() {
        let mut cache = Cache::default();
        cache.plugins.insert(
            "apps".into(),
            ["Code", "Visual Studio Code", "微信"]
                .iter()
                .enumerate()
                .map(|(i, title)| Entry {
                    id: i.to_string(),
                    title: title.to_string(),
                    subtitle: String::new(),
                    keywords: vec![],
                    target: Target::Url {
                        url: "https://example.com".into(),
                    },
                })
                .collect(),
        );
        let mut settings = Settings::default();
        let index = build_index(&cache, &settings);
        settings.usage.insert(
            "apps:0".into(),
            Usage {
                count: 101,
                last_used: now(),
            },
        );
        settings.usage.insert(
            "apps:1".into(),
            Usage {
                count: 102,
                last_used: 1,
            },
        );
        settings.usage.insert(
            "apps:2".into(),
            Usage {
                count: 10000,
                last_used: now(),
            },
        );
        let titles = |query: &str, settings: &Settings| {
            search(&index, query, settings, 50)
                .iter()
                .map(|&i| index[i].entry.title.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("code", &settings), ["Visual Studio Code", "Code"]);
        assert_eq!(titles("visual code", &settings), ["Visual Studio Code"]);
        assert_eq!(titles("wx", &settings), ["微信"]);
        assert_eq!(
            titles("", &settings),
            ["微信", "Visual Studio Code", "Code"]
        );
        settings.usage.get_mut("apps:0").unwrap().count = 102;
        assert_eq!(titles("code", &settings), ["Code", "Visual Studio Code"]);
        settings.usage.clear();
        assert_eq!(titles("code", &settings), ["Code", "Visual Studio Code"]);
    }
    #[test]
    fn chinese_pinyin_initials_and_usage() {
        let entries = ["微信", "Visual Studio Code", "计算器"]
            .iter()
            .enumerate()
            .map(|(i, name)| Entry {
                id: i.to_string(),
                title: name.to_string(),
                subtitle: String::new(),
                keywords: vec![],
                target: Target::Url {
                    url: "https://example.com".into(),
                },
            })
            .collect();
        let mut cache = Cache::default();
        cache.plugins.insert("apps".into(), entries);
        let mut settings = Settings::default();
        let index = build_index(&cache, &settings);
        for query in ["微信", "weixin", "wx"] {
            assert_eq!(
                index[search(&index, query, &settings, 10)[0]].entry.title,
                "微信"
            );
        }
        assert_eq!(
            index[search(&index, "visual code", &settings, 10)[0]]
                .entry
                .title,
            "Visual Studio Code"
        );
        assert!(search(&index, "notfound", &settings, 10).is_empty());
        settings.usage.insert(
            "apps:2".into(),
            Usage {
                count: 10,
                last_used: now(),
            },
        );
        assert_eq!(
            index[search(&index, "", &settings, 10)[0]].entry.title,
            "计算器"
        );
        settings.disabled_plugins.insert("apps".into());
        assert!(build_index(&cache, &settings).is_empty());
    }
}
