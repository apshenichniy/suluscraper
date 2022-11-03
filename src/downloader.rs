#![allow(dead_code)]

use reqwest::Url;

#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    tries: usize,
}
impl Downloader {
    pub fn new(tries: usize) -> Self {
        Downloader {
            client: reqwest::Client::new(),
            tries,
        }
    }

    pub async fn get(&self, url: &Url) -> Result<String, reqwest::Error> {
        let mut error: Option<reqwest::Error> = None;

        for _ in 0..self.tries {
            match self.client.get(url.clone()).send().await {
                Ok(response) => return response.text().await,
                Err(err) => error = Some(err),
            }
        }
        Err(error.unwrap())
    }
}
