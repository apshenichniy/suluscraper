#![allow(dead_code, unused_variables, unused_mut)]
use std::path::PathBuf;

use suluscraper::{
    args::{Args, ScrapeTarget},
    scraper::Scraper,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // mock args
    let args = Args {
        target: ScrapeTarget::Ulta,
        output: PathBuf::from("/Users/apshenichniy/Downloads/json"),
        jobs: 4,
        tries: 3,
        delay: 0,
    };

    let scraper = Scraper::new(args);
    scraper.run().await;

    Ok(())
}
