use std::fmt;
use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser as _;
use futures::{StreamExt, TryStreamExt};

#[derive(clap::Parser)]
struct Command {
    event_handle: Option<String>,
    event_edition: Option<u32>,
    #[arg(long, short, default_value_t = String::from("./"))]
    out: String,
}

#[derive(serde::Deserialize)]
struct Map {
    mx_id: i64,
    map_uid: String,
}

impl fmt::Display for Map {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (MX ID: {})", self.map_uid, self.mx_id)
    }
}

#[derive(serde::Deserialize)]
struct Category {
    handle: String,
    maps: Vec<Map>,
}

#[derive(serde::Deserialize)]
struct EventEdition {
    name: String,
    mx_id: i32,
    categories: Vec<Category>,
}

impl fmt::Display for EventEdition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (MX ID: {})", self.name, self.mx_id)
    }
}

#[tracing::instrument(skip(client), err, ret(Display))]
async fn get_event_edition(
    client: &reqwest::Client,
    handle: &str,
    edition: u32,
) -> anyhow::Result<EventEdition> {
    #[cfg(feature = "localhost_test")]
    let url = format!("http://localhost:3001/event/{}/{}", handle, edition);
    #[cfg(not(feature = "localhost_test"))]
    let url = format!(
        "https://obstacle.titlepack.io/api/event/{}/{}",
        handle, edition
    );
    tracing::info!("Requesting event edition at {url}...");
    client
        .get(&url)
        .send()
        .await
        .context("Failed to send request")?
        .json()
        .await
        .context("Failed to parse JSON from response")
}

#[tracing::instrument(skip(client), fields(map = %map), err)]
async fn download_map(
    client: &reqwest::Client,
    map: Map,
) -> anyhow::Result<(String, bytes::Bytes)> {
    tracing::info!("Downloading map...");

    let url = format!("https://sm.mania.exchange/maps/download/{}", map.mx_id);
    Ok((
        map.map_uid,
        client
            .get(url)
            .header("User-Agent", "obstacle (discord @ahmadbky)")
            .send()
            .await?
            .bytes()
            .await
            .context("Unable to get bytes from response body")?,
    ))
}

#[tracing::instrument(skip(client, cat), fields(cat.handle = %cat.handle), err)]
async fn download_category(
    client: &reqwest::Client,
    cat: Category,
) -> anyhow::Result<(String, Vec<(String, bytes::Bytes)>)> {
    tracing::info!("Downloading category's maps...");

    let maps_len = cat.maps.len();

    Ok((
        cat.handle,
        futures::stream::iter(cat.maps)
            .map(|map| download_map(client, map))
            .buffer_unordered(maps_len)
            .try_collect::<Vec<_>>()
            .await
            .context("Unable to collect maps downloads")?,
    ))
}

#[derive(serde::Deserialize)]
struct SimpleEventEdition {
    id: u32,
    name: String,
}

impl fmt::Display for SimpleEventEdition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Event edition `{}` (Edition ID: {})", self.name, self.id)
    }
}

#[tracing::instrument(skip(client, event_handle), err, ret(Display))]
async fn get_last_edition_of(
    client: &reqwest::Client,
    event_handle: &str,
) -> anyhow::Result<SimpleEventEdition> {
    #[cfg(feature = "localhost_test")]
    let url = format!("http://localhost:3001/event/{event_handle}");
    #[cfg(not(feature = "localhost_test"))]
    let url = format!("https://obstacle.titlepack.io/api/event/{event_handle}");

    tracing::info!("Requesting event editions at {url}...");

    Ok(
        client
            .get(&url)
            .send()
            .await?
            .json::<Vec<SimpleEventEdition>>()
            .await
            .context("Unable to parse JSON response for event editions")?
            .into_iter()
            .max_by_key(|o| o.id)
            .unwrap(), // The event must have at least one edition
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Command::parse();

    tracing_subscriber::fmt().compact().init();
    let c = reqwest::Client::new();

    let out_path = PathBuf::from(args.out);
    let (event_handle, event_edition) = match (args.event_handle, args.event_edition) {
        (Some(event), Some(edition)) => (event, edition),
        (Some(event), None) => {
            tracing::info!("Provided `{event}` event, querying last edition...");
            let edition = get_last_edition_of(&c, &event).await?;
            (event, edition.id)
        }
        (None, Some(_)) => {
            anyhow::bail!("Cannot provide an edition ID without an event handle");
        }
        (None, None) => {
            tracing::info!("No parameter provided, querying last edition of campaign...");
            (
                "campaign".to_owned(),
                get_last_edition_of(&c, "campaign").await?.id,
            )
        }
    };

    let event = get_event_edition(&c, &event_handle, event_edition)
        .await
        .context("Failed to get event edition")?;

    tracing::info!("Downloading content from MX...");

    let cats_len = event.categories.len();
    let mut cats = futures::stream::iter(event.categories)
        .map(|cat| download_category(&c, cat))
        .buffer_unordered(cats_len);

    let out_path = out_path.join(event_handle).join(event_edition.to_string());

    while let Some(cat) = cats.next().await {
        let (cat_handle, maps) = cat?;
        tracing::info!("Writing maps of category `{cat_handle}`");
        let cat_dir = out_path.join(cat_handle);
        std::fs::create_dir_all(&cat_dir).context("Unable to create directory")?;
        for (map_uid, content) in maps {
            std::fs::write(cat_dir.join(format!("{map_uid}.Map.Gbx")), content)
                .context("Unable to write map file")?;
        }
    }

    Ok(())
}
