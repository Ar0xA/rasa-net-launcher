use serde::Serialize;

const NEWS_FEED: &str = "https://infiniterasa.org/news/rss.xml";
const MAX_PER_KIND: usize = 8;

#[derive(Serialize)]
pub struct NewsItem {
    title: String,
    link: String,
    summary: String,
    /// Unix milliseconds, 0 when the feed date is missing or unparseable.
    date: i64,
    /// Post type from the feed: Announcement, Patch notes, Community, Event, Dev blog.
    category: String,
    /// True for posts in the feed's "Patch notes" category.
    patch_notes: bool,
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Latest posts from the site's RSS feed, newest first.
pub async fn fetch() -> Result<Vec<NewsItem>, String> {
    let bytes = reqwest::Client::new()
        .get(NEWS_FEED)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let channel = rss::Channel::read_from(&bytes[..]).map_err(|e| e.to_string())?;

    let mut items: Vec<NewsItem> = channel
        .items()
        .iter()
        .map(|i| NewsItem {
            category: i.categories().first().map(|c| c.name().to_owned()).unwrap_or_default(),
            title: i.title().unwrap_or("Untitled").to_owned(),
            link: i.link().unwrap_or_default().to_owned(),
            summary: strip_html(i.description().unwrap_or_default()),
            date: i
                .pub_date()
                .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
                .map_or(0, |d| d.timestamp_millis()),
            patch_notes: i
                .categories()
                .iter()
                .any(|c| c.name().to_lowercase().contains("patch")),
        })
        .collect();
    items.sort_by(|a, b| b.date.cmp(&a.date));

    // Keep the newest few of each kind so a burst of one never hides the other.
    let (mut news, mut patches) = (0, 0);
    items.retain(|i| {
        let count = if i.patch_notes { &mut patches } else { &mut news };
        *count += 1;
        *count <= MAX_PER_KIND
    });
    Ok(items)
}
