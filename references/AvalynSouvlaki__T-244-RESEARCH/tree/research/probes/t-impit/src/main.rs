use impit::cookie::Jar;
use impit::{impit::Impit, fingerprint::database as fp};
use scraper::{Html, Selector};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args().nth(1).unwrap_or("https://example.com".into());
    let imp = Impit::<Jar>::builder()
        .with_fingerprint(fp::chrome_151::fingerprint())
        .build()?;
    let resp = imp.get(url.clone(), None, None).await?;
    let status = resp.status();
    let body = resp.text().await?;
    eprintln!("status={} bytes={}", status, body.len());

    // T-244 shape: every href ending .torrent, every magnet: URI, + anchor text
    let doc = Html::parse_document(&body);
    let sel = Selector::parse("a[href]").unwrap();
    let mut hits = 0;
    for el in doc.select(&sel) {
        let href = el.value().attr("href").unwrap_or("");
        if href.ends_with(".torrent") || href.starts_with("magnet:") {
            let text: String = el.text().collect::<String>().trim().to_string();
            println!("{href}\t{text}");
            hits += 1;
        }
    }
    eprintln!("matches={hits}");
    Ok(())
}
