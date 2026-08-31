use wreq::Client;
use wreq_util::Emulation;
use scraper::{Html, Selector};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args().nth(1).unwrap_or("https://example.com".into());
    let client = Client::builder().emulation(Emulation::Chrome136).build()?;
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    eprintln!("status={} bytes={}", status, body.len());
    let doc = Html::parse_document(&body);
    let sel = Selector::parse("a[href]").unwrap();
    let mut hits = 0;
    for el in doc.select(&sel) {
        let href = el.value().attr("href").unwrap_or("");
        if href.ends_with(".torrent") || href.starts_with("magnet:") {
            println!("{href}\t{}", el.text().collect::<String>().trim());
            hits += 1;
        }
    }
    eprintln!("matches={hits}");
    Ok(())
}
