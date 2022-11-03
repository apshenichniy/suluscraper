#![allow(dead_code, unused_variables)]

use crate::downloader::Downloader;

use super::args;
use super::downloader;
use super::parser::*;
use indicatif::{HumanDuration, MultiProgress, ProgressBar};
use scraper::Html;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub struct Scraper {
    args: args::Args,
}

impl Scraper {
    pub fn new(args: args::Args) -> Self {
        Scraper { args }
    }

    fn new_parser(&self) -> Box<dyn ProductParser> {
        let parser_type = &self.args.target;
        let parser = match parser_type {
            args::ScrapeTarget::Ulta => UltaParser::new(),
            args::ScrapeTarget::Amazon => todo!(),
            args::ScrapeTarget::Walgreens => todo!(),
            args::ScrapeTarget::Walmart => todo!(),
            args::ScrapeTarget::Macys => todo!(),
            args::ScrapeTarget::Target => todo!(),
            args::ScrapeTarget::Marshalls => todo!(),
            args::ScrapeTarget::Cvs => todo!(),
        };

        Box::new(parser)
    }

    fn new_downloader(&self) -> Downloader {
        downloader::Downloader::new(self.args.tries)
    }

    pub async fn run(&self) {
        let started = Instant::now();

        // channel for getting product page job queue
        let (jobs_sender, jobs_reciever) = async_channel::unbounded::<ProductShortDetails>();

        let parser = self.new_parser();
        let downloader = self.new_downloader();
        let m = MultiProgress::new();
        let pb = m.add(ProgressBar::new_spinner());
        let delay = self.args.delay;

        let handle = tokio::spawn(async move {
            pb.enable_steady_tick(Duration::from_millis(20));

            let mut pages_count = 0;
            let mut current_page_url = parser.get_first_page();

            loop {
                pb.set_message(format!("get {}", current_page_url.as_str()));

                let body = downloader
                    .get(&current_page_url)
                    .await
                    .expect(format!("Error getting URL {}", current_page_url).as_str());

                // enclose parsing tasks in separate scope to ensure that not thread-safe variables will be dropped before .await call
                let (products, finished) = {
                    let document = Html::parse_document(body.as_str());
                    // parse products on page
                    let products = parser.get_product_short_details(&document);
                    let mut finished = false;

                    match parser.get_next_page(&document) {
                        Some(next_page_url) => current_page_url = next_page_url,
                        None => finished = true,
                    }
                    (products, finished)
                };

                // send products to job queue
                for item in products {
                    if let Err(e) = jobs_sender.send(item).await {
                        pb.set_message(format!("Job sender has encountered an error: {}", e));
                    }
                }

                pages_count += 1;

                if finished {
                    break;
                }
                std::thread::sleep(Duration::from_secs(delay));
            }
            pb.finish_with_message(format!("pages done [{}]", pages_count));
        });

        // create output directory
        std::fs::create_dir_all(self.args.output.as_path()).unwrap();

        // create mpsc channel for product data results
        let (res_sender, mut res_reciever) = mpsc::unbounded_channel::<ProductDetails>();

        // spawn workers
        let mut jobs = vec![];

        for w in 0..self.args.jobs {
            let res_sender = res_sender.clone();
            let jobs_reciever = jobs_reciever.clone();
            let downloader = self.new_downloader();
            let parser = self.new_parser();
            let delay = self.args.delay;

            let pb = m.add(ProgressBar::new_spinner());
            pb.enable_steady_tick(Duration::from_millis(20));
            pb.set_message("waiting...");
            let path = self.args.output.clone();

            let job_handle = tokio::spawn(async move {
                let mut count = 0;
                loop {
                    match jobs_reciever.recv().await {
                        Ok(job) => {
                            pb.set_message(format!("done [{}] get {}", count, &job.url.as_str()));
                            // download and parse
                            match downloader.get(&job.url).await {
                                Ok(body) => {
                                    let document = Html::parse_document(body.as_str());
                                    let details = parser.get_product_details(&document, &job.url);

                                    match res_sender.send(details) {
                                        Ok(()) => {}
                                        Err(e) => pb.set_message(format!("error: {}", e)),
                                    }
                                }
                                Err(e) => {
                                    pb.set_message(format!("error: {}", e));
                                }
                            };
                            std::thread::sleep(Duration::from_secs(delay));
                        }
                        Err(_) => break,
                    }
                    count += 1;
                }
                pb.finish_with_message("waiting...");
            });
            jobs.push(job_handle);
        }

        std::mem::drop(res_sender);

        // create output directory
        std::fs::create_dir_all(self.args.output.as_path()).unwrap();

        // process results queue
        let pb = m.add(ProgressBar::new_spinner());
        pb.enable_steady_tick(Duration::from_millis(20));
        pb.set_message("waiting...");

        let mut i = 0;
        loop {
            match res_reciever.recv().await {
                Some(details) => {
                    let mut path = self.args.output.clone();
                    path.push(PathBuf::from(format!("product_{}.json", i)));
                    pb.set_message(format!(
                        "done [{}] write {}",
                        i,
                        &path.as_os_str().to_str().unwrap()
                    ));
                    serde_json::to_writer_pretty(&std::fs::File::create(path).unwrap(), &details)
                        .unwrap();
                }
                None => {
                    pb.finish_with_message(format!("done [{}] COMPLETE", i));
                    break;
                }
            }
            i += 1;
        }
        m.clear().unwrap();

        println!("DONE in {}", HumanDuration(started.elapsed()));
    }
}
