use anyhow::Context;

use crate::{EventEdition, SimpleEventEdition};

#[tracing::instrument(skip(client), err, ret(Display))]
pub async fn get_event_edition(
    client: &reqwest::Client,
    host: &str,
    handle: &str,
    edition: u32,
) -> anyhow::Result<EventEdition> {
    let url = format!("{host}/event/{handle}/{edition}");
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

#[tracing::instrument(skip(client, event_handle), err, ret(Display))]
pub async fn get_last_edition_of(
    client: &reqwest::Client,
    host: &str,
    event_handle: &str,
) -> anyhow::Result<SimpleEventEdition> {
    let url = format!("{host}/event/{event_handle}");

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
