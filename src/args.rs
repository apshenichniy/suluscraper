use std::path::PathBuf;

use serde::Serialize;

pub struct Args {
    /// What do you want to scrape?
    pub target: ScrapeTarget,

    /// Output directory
    pub output: PathBuf,

    /// Maximum number of threads to use concurrently
    pub jobs: usize,

    /// Maximum amount of retries on download failure
    pub tries: usize,

    /// Add a delay in seconds between dowload to reduce the likelyhood of getting banned
    pub delay: u64,
}

#[derive(Serialize, Debug)]
pub enum ScrapeTarget {
    Ulta,
    Amazon,
    Walgreens,
    Walmart,
    Macys,
    Target,
    Marshalls,
    Cvs,
}
